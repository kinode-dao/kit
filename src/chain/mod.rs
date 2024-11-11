use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use color_eyre::{
    eyre::{eyre, Result},
    Section,
};
use fs_err as fs;
use reqwest::Client;
use tokio::time::{sleep, Duration};
use tracing::{info, instrument};

use crate::run_tests::cleanup::{clean_process_by_pid, cleanup_on_signal};
use crate::run_tests::types::BroadcastRecvBool;
use crate::setup::{check_foundry_deps, get_deps};
use crate::KIT_CACHE;

include!("../../target/chain_includes.rs");

const DEFAULT_MAX_ATTEMPTS: u16 = 16;

pub const FAKENODE_TO_FOUNDRY: &[(&str, &str)] = &[("<0.9.8", "008922d51"), (">=0.9.8", "c3069a5")];
pub const FOUNDRY_COMMIT_TO_DATE: &[(&str, &str)] = &[
    ("008922d51", "2024-04-23T00:23:10.634984900Z"),
    ("c3069a5", "2024-11-05T00:22:10.561306811Z"),
];
pub const FOUNDRY_NEWEST_COMMIT: &str = "c3069a5";

#[instrument(level = "trace", skip_all)]
pub async fn start_chain(
    port: u16,
    mut recv_kill: BroadcastRecvBool,
    fakenode_version: Option<semver::Version>,
    verbose: bool,
) -> Result<Option<Child>> {
    let fakenode_to_foundry: HashMap<semver::VersionReq, String> = FAKENODE_TO_FOUNDRY
        .iter()
        .map(|ss| (ss.0.parse().unwrap(), ss.1.to_string()))
        .collect();
    let foundry_commit_to_date: HashMap<String, chrono::DateTime<chrono::Utc>> =
        FOUNDRY_COMMIT_TO_DATE
            .iter()
            .map(|ss| (ss.0.to_string(), ss.1.parse().unwrap()))
            .collect();
    let foundry_commit_to_content: HashMap<String, String> = FOUNDRY_COMMIT_TO_CONTENT
        .iter()
        .map(|ss| (ss.0.to_string(), ss.1.to_string()))
        .collect();

    let (newer_than, required_commit) = match fakenode_version {
        None => (None, None),
        Some(v) => {
            let Some((_, commit)) = fakenode_to_foundry.iter().find(|(vr, _)| vr.matches(&v))
            else {
                return Err(eyre!(""));
            };
            (
                foundry_commit_to_date.get(commit).map(|d| d.clone()),
                Some(commit.to_string()),
            )
        }
    };

    let deps = check_foundry_deps(newer_than, required_commit.clone())?;
    get_deps(deps, &mut recv_kill, verbose).await?;

    let required_commit = required_commit.unwrap_or_else(|| FOUNDRY_NEWEST_COMMIT.to_string());

    let kinostate_path = PathBuf::from(KIT_CACHE).join(format!("kinostate-{required_commit}.json"));
    let kinostate_content = foundry_commit_to_content
        .get(&required_commit)
        .expect(&format!(
            "couldn't find kinostate content for foundry commit {required_commit}"
        ));
    fs::write(&kinostate_path, kinostate_content)?;

    info!("Checking for Anvil on port {}...", port);
    if wait_for_anvil(port, 1, None).await.is_ok() {
        return Ok(None);
    }

    let mut child = Command::new("anvil")
        .arg("--port")
        .arg(port.to_string())
        .arg("--load-state")
        .arg(&kinostate_path)
        .current_dir(KIT_CACHE)
        .stdout(if verbose {
            Stdio::inherit()
        } else {
            Stdio::piped()
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
pub async fn execute(port: u16, version: &str, verbose: bool) -> Result<()> {
    let (send_to_cleanup, mut recv_in_cleanup) = tokio::sync::mpsc::unbounded_channel();
    let (send_to_kill, _recv_kill) = tokio::sync::broadcast::channel(1);
    let recv_kill_in_cos = send_to_kill.subscribe();

    let handle_signals = tokio::spawn(cleanup_on_signal(send_to_cleanup.clone(), recv_kill_in_cos));

    let recv_kill_in_start_chain = send_to_kill.subscribe();
    let version = if version == "latest" {
        None
    } else {
        Some(version.parse()?)
    };
    let child = start_chain(port, recv_kill_in_start_chain, version, verbose).await?;
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
