use color_eyre::eyre::{eyre, Result};
use std::net::TcpListener;
use std::process::{Child, Command};
use fs_err as fs;

use crate::{KIT_KINOSTATE_PATH_DEFAULT, KIT_CACHE};

pub const KINOSTATE_JSON: &str = include_str!("./kinostate.json");

pub async fn fetch_kinostate() -> Result<()> {
    let json_path = KIT_KINOSTATE_PATH_DEFAULT;

    if fs::metadata(json_path).is_ok() {
    } else {
        fs::write(&json_path, KINOSTATE_JSON)?;
    }
    Ok(())
}

pub fn start_chain(port: u16) -> Result<Child> {
    let child = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => {
            let child = Command::new("anvil")
                .arg("--port")
                .arg(port.to_string())
                .arg("--load-state")
                .arg(KIT_KINOSTATE_PATH_DEFAULT)
                .stdout(std::process::Stdio::piped())
                .spawn()?;
            Ok(child)
        }
        Err(e) => Err(eyre!("Port {} is already in use: {}", port, e)),
    };

    // TODO: read stdout to know when anvil is ready instead.
    std::thread::sleep(std::time::Duration::from_millis(100));
    child
}

/// kit chain, alias to anvil
pub async fn execute(port: u16) -> Result<()> {
    fetch_kinostate().await?;

    Command::new("anvil")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(KIT_CACHE)
        .arg("--load-state")
        .arg("./kinostate.json")
        .spawn()?;

    Ok(())
}