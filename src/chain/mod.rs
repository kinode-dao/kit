use color_eyre::eyre::{eyre, Result};
use fs_err as fs;
use sha2::{Sha256, Digest};
use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::run_tests::cleanup::{cleanup_on_signal, clean_process_by_pid};
use crate::KIT_CACHE;

pub const KINOSTATE_JSON: &str = include_str!("./kinostate.json");

pub async fn fetch_kinostate() -> Result<String> {
    let state_hash = {
        let mut hasher = Sha256::new();
        hasher.update(KINOSTATE_JSON.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    let json_path = format!("{}/kinostate-{}.json", KIT_CACHE, state_hash);

    if fs::metadata(&json_path).is_ok() {
    } else {
        fs::write(&json_path, KINOSTATE_JSON)?;
    }
    Ok(state_hash)
}

pub async fn start_chain(port: u16, state_hash: &str) -> Result<Child> {
    let state_path = format!("{}/kinostate-{}.json", KIT_CACHE, state_hash);

    let child = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(_) => {
            let mut child = Command::new("anvil")
                .arg("--port")
                .arg(port.to_string())
                .arg("--load-state")
                .arg(state_path)
                .stdout(std::process::Stdio::piped())
                .spawn()?;

            let stdout = child.stdout.take().ok_or_else(|| eyre!("Failed to capture stdout"))?;
            let mut reader = BufReader::new(stdout).lines();

            tokio::spawn(async move {
                while let Some(line) = reader.next_line().await? {
                    if line.contains("Listening") {
                        println!("Spawned anvil fakechain at port: {}", port);
                        break;
                    }
                }
                Ok::<_, std::io::Error>(())
            });

            Ok(child)
        }
        Err(e) => Err(eyre!("Port {} is already in use: {}", port, e)),
    };

    std::thread::sleep(std::time::Duration::from_secs(1));
    child
}

/// kit chain, alias to anvil
pub async fn execute(port: u16) -> Result<()> {
    let state_hash = fetch_kinostate().await?;
    let state_path = format!("./kinostate-{}.json", state_hash);

    let mut child = Command::new("anvil")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(KIT_CACHE)
        .arg("--load-state")
        .arg(state_path)
        .spawn()?;    
    let child_id = child.id().unwrap() as i32;

    let (send_to_cleanup, mut recv_in_cleanup) = tokio::sync::mpsc::unbounded_channel();
    let (send_to_kill, _recv_kill) = tokio::sync::broadcast::channel(1);
    let recv_kill_in_cos = send_to_kill.subscribe();

    let handle_signals = tokio::spawn(cleanup_on_signal(send_to_cleanup.clone(), recv_kill_in_cos));

    let cleanup_anvil = tokio::spawn(async move {
        let status = child.wait().await;
        clean_process_by_pid(child_id);
        status
    });

    tokio::select! {
        _ = handle_signals => {}
        _ = cleanup_anvil => {}
        _ = recv_in_cleanup.recv() => {}
    }

    let _ = send_to_kill.send(true);

    Ok(())
}
