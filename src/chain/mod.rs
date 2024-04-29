use color_eyre::eyre::{eyre, Result};
use std::net::TcpListener;
use std::process::{Child, Command};

pub mod register;

pub use register::RegisterHelpers::*;
pub use register::*;

use crate::KIT_CACHE;

pub fn start_chain(port: u16) -> Result<Child> {
    let child = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => {
            let child = Command::new("anvil")
                .arg("--port")
                .arg(port.to_string())
                .arg("--load-state")
                .arg(format!("{}/kinostate.json", KIT_CACHE))
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
    Command::new("anvil")
        .arg("--port")
        .arg(port.to_string())
        .stdout(std::process::Stdio::piped())
        // .arg("--load-state")
        // .arg("./kinostate.json")
        .spawn()?;

    Ok(())
}
