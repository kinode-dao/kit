use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

use color_eyre::{eyre::eyre, Result, Section};
use fs_err as fs;
use tracing::{info, instrument};

use crate::KIT_CACHE;

const MIN_PORT: u16 = 8080;
const MAX_PORT: u16 = 8999;

#[instrument(level = "trace", skip_all)]
fn extract_port(input: &str) -> Vec<u16> {
    let mut ports = vec![];
    // Iterate through each line in the input string
    for line in input.lines() {
        // Check if the line contains a valid port number within the specified range
        for word in line.split_whitespace() {
            // Attempt to parse the word as a u16
            if let Ok(port) = word.trim_start_matches("*:").parse::<u16>() {
                // Check if the port is within the desired range
                if port >= MIN_PORT && port <= MAX_PORT {
                    ports.push(port);
                }
            }
        }
    }
    ports
}

#[instrument(level = "trace", skip_all)]
fn get_port(host: &str) -> Result<u16> {
    let output = Command::new("bash")
        .args(&["-c", &format!("ssh {host} lsof -i -P -n")])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("Failed to `ssh {host}`: {stderr}"));
    }
    let stdout = String::from_utf8(output.stdout)?;
    let ports = extract_port(&stdout);
    if ports.len() != 1 {
        info!("{stdout}");
        return Err(eyre!(
            "couldn't find specific port for ssh amongst: {ports:?}"
        ));
    }
    let port = ports[0];

    Ok(port)
}

#[instrument(level = "trace", skip_all)]
fn is_port_available(bind_addr: &str) -> bool {
    TcpListener::bind(bind_addr).is_ok()
}

#[instrument(level = "trace", skip_all)]
fn start_tunnel(local_port: u16, host: &str, host_port: u16) -> Result<u32> {
    let command = format!("ssh -L {local_port}:localhost:{host_port} {host} -N -f");
    let output = Command::new("bash").args(["-c", &command]).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("Failed to open ssh tunnel: {stderr}"));
    }
    let output = Command::new("bash")
        .args([
            "-c",
            &format!("ps -ef | grep '{command}' | grep -v grep | awk '{{print $2}}'"),
        ])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("Failed to get ssh tunnel PID: {stderr}"));
    }
    let stdout = String::from_utf8(output.stdout)?;
    let pid = stdout.trim().parse()?;

    Ok(pid)
}

/// Store `pid`, keyed by `local_port`, for use by `kit disconnect`
#[instrument(level = "trace", skip_all)]
fn write_pid_to_file(local_port: u16, pid: u32) -> Result<()> {
    let kit_cache = std::path::PathBuf::from(KIT_CACHE);
    let connect_path = kit_cache.join("connect");
    if !connect_path.exists() {
        std::fs::create_dir_all(&connect_path)?;
    }
    fs::write(
        connect_path.join(format!("{}", local_port)),
        format!("{pid}"),
    )?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn make_pid_file_path(local_port: &u16) -> Result<PathBuf> {
    let kit_cache = std::path::PathBuf::from(KIT_CACHE);
    let pid_file_path = kit_cache.join("connect").join(format!("{local_port}"));
    if !pid_file_path.exists() {
        return Err(eyre!("pid file {pid_file_path:?} doesn't exist"));
    }
    Ok(pid_file_path)
}

#[instrument(level = "trace", skip_all)]
fn read_pid_from_file(local_port: u16) -> Result<u32> {
    let pid_file_path = make_pid_file_path(&local_port)?;
    let port = std::fs::read_to_string(pid_file_path)?;
    let port = port.parse()?;
    Ok(port)
}

#[instrument(level = "trace", skip_all)]
fn kill_pid(pid: i32) -> Result<()> {
    let pid = nix::unistd::Pid::from_raw(pid);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT)?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn disconnect(local_port: u16) -> Result<()> {
    if is_port_available(&format!("127.0.0.1:{local_port}")) {
        return Err(eyre!(
            "given local port {local_port} is not occupied: nothing to disconnect"
        ));
    }

    let pid = read_pid_from_file(local_port)
        .map_err(|e| e.with_suggestion(|| format!("To disconnect, try\n```\nps -ef | grep -e PID -e 'ssh -L.*{local_port}' | grep -v grep\n```\nto identify `ssh -L` tunnel PID then use `kill <PID>`")))?;
    kill_pid(pid as i32)?;
    let pid_file_path = make_pid_file_path(&local_port)?;
    fs::remove_file(pid_file_path)?;

    Ok(())
}

/// user & host required only for disconnect == false
#[instrument(level = "trace", skip_all)]
pub fn execute(
    local_port: u16,
    is_disconnect: bool,
    host: Option<&str>,
    host_port: Option<u16>,
) -> Result<()> {
    if is_disconnect {
        info!("Disconnecting tunnel on {local_port}...");
        disconnect(local_port)?;
        info!("Done disconnecting tunnel on {local_port}.");
        return Ok(());
    }

    let Some(host) = host else {
        return Err(eyre!(
            "host is a required field when connecting a new tunnel."
        ));
    };
    info!("Connecting tunnel on {local_port} to {host}...");

    // connect: create connection
    if !is_port_available(&format!("127.0.0.1:{local_port}")) {
        return Err(eyre!("given local port {local_port} occupied")
            .with_suggestion(|| "try binding an open one"));
    }

    let host_port = host_port
        .map(|hp| Ok(hp)) // enable the `?`
        .unwrap_or_else(|| get_port(host))?;

    let pid = start_tunnel(local_port, host, host_port)?;
    write_pid_to_file(local_port, pid)?;

    info!("Done connecting tunnel on {local_port} to {host}. Disconnect by running\n```\nkit connect -p {local_port} -d\n```");
    Ok(())
}
