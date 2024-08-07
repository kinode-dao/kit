use std::process::{Child, Command, Stdio};

use color_eyre::{
    eyre::{eyre, Result},
    Section,
};
use fs_err as fs;
use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::time::{sleep, Duration};
use tracing::{info, instrument};

use crate::run_tests::cleanup::{clean_process_by_pid, cleanup_on_signal};
use crate::run_tests::types::BroadcastRecvBool;
use crate::setup::{check_foundry_deps, get_deps};
use crate::KIT_CACHE;

const KINOSTATE_JSON: &str = include_str!("./kinostate.json");
const DEFAULT_MAX_ATTEMPTS: u16 = 16;

#[instrument(level = "trace", skip_all)]
async fn write_kinostate() -> Result<String> {
    let state_hash = {
        let mut hasher = Sha256::new();
        hasher.update(KINOSTATE_JSON.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    let json_path = format!("{}/kinostate-{}.json", KIT_CACHE, state_hash);
    let json_path = std::path::PathBuf::from(json_path);

    if !json_path.exists() {
        fs::write(&json_path, KINOSTATE_JSON)?;
    }
    Ok(state_hash)
}

#[instrument(level = "trace", skip_all)]
pub async fn start_chain(
    port: u16,
    piped: bool,
    recv_kill: BroadcastRecvBool,
    verbose: bool,
) -> Result<Option<Child>> {
    let deps = check_foundry_deps()?;
    get_deps(deps, verbose)?;

    let state_hash = write_kinostate().await?;
    let state_path = format!("./kinostate-{}.json", state_hash);

    info!("Checking for Anvil on port {}...", port);
    if wait_for_anvil(port, 1, None).await.is_ok() {
        return Ok(None);
    }

    let mut child = Command::new("anvil")
        .arg("--port")
        .arg(port.to_string())
        .arg("--load-state")
        .arg(&state_path)
        .current_dir(KIT_CACHE)
        .stdout(if piped {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .spawn()?;

    info!("Waiting for Anvil to be ready on port {}...", port);
    if let Err(e) = wait_for_anvil(port, DEFAULT_MAX_ATTEMPTS, Some(recv_kill)).await {
        let _ = child.kill();
        return Err(e);
    }

    Ok(Some(child))
}

#[instrument(level = "trace", skip_all)]
async fn wait_for_anvil(
    port: u16,
    max_attempts: u16,
    mut recv_kill: Option<BroadcastRecvBool>,
) -> Result<()> {
    let client = Client::new();
    let url = format!("http://localhost:{}", port);

    for _ in 0..max_attempts {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let response = client.post(&url).json(&request_body).send().await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let result: serde_json::Value = resp.json().await?;
                if let Some(block_number) = result["result"].as_str() {
                    if block_number.starts_with("0x") {
                        info!("Anvil is ready on port {}.", port);
                        return Ok(());
                    }
                }
            }
            _ => (),
        }

        if let Some(ref mut recv_kill) = recv_kill {
            tokio::select! {
                _ = sleep(Duration::from_millis(250)) => {}
                _ = recv_kill.recv() => {
                    return Err(eyre!("Received kill: bringing down anvil."));
                }
            }
        } else {
            sleep(Duration::from_millis(250)).await;
        }
    }

    Err(eyre!(
        "Failed to connect to Anvil on port {} after {} attempts",
        port,
        max_attempts
    )
    .with_suggestion(|| "Is port already occupied?"))
}

/// kit chain, alias to anvil
#[instrument(level = "trace", skip_all)]
pub async fn execute(port: u16, verbose: bool) -> Result<()> {
    let (send_to_cleanup, mut recv_in_cleanup) = tokio::sync::mpsc::unbounded_channel();
    let (send_to_kill, _recv_kill) = tokio::sync::broadcast::channel(1);
    let recv_kill_in_cos = send_to_kill.subscribe();

    let handle_signals = tokio::spawn(cleanup_on_signal(send_to_cleanup.clone(), recv_kill_in_cos));

    let recv_kill_in_start_chain = send_to_kill.subscribe();
    let child = start_chain(port, false, recv_kill_in_start_chain, verbose).await?;
    let Some(mut child) = child else {
        return Err(eyre!(
            "Port {} is already in use by another anvil process",
            port
        ));
    };
    let child_id = child.id() as i32;

    let cleanup_anvil = tokio::spawn(async move {
        recv_in_cleanup.recv().await;
        clean_process_by_pid(child_id);
    });

    let _ = child.wait();

    let _ = handle_signals.await;
    let _ = cleanup_anvil.await;

    let _ = send_to_kill.send(true);

    Ok(())
}
