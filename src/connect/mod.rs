use std::collections::HashMap;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};

use color_eyre::{eyre::eyre, Result, Section};
use fs_err as fs;
use procfs::net::{tcp, tcp6};
use procfs::process::{all_processes, FDTarget};
use ssh2::Session;
use tracing::instrument;

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
            if let Ok(port) = word.trim_start_matches('*').parse::<u16>() {
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
fn get_port(user: &str, host: &str) -> Result<u16> {
    // Establish TCP connection
    let tcp = TcpStream::connect(format!("{host}:22"))?;

    // Create SSH session
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;

    // Attempt to authenticate using ssh-agent
    if let Ok(mut agent) = sess.agent() {
        agent.connect()?;
        agent.list_identities()?;

        for identity in agent.identities()? {
            if agent.userauth(user, &identity).is_ok() {
                break;
            }
        }
    }

    if !sess.authenticated() {
        // If ssh-agent failed, prompt for password
        print!("Password: ");
        std::io::stdout().flush()?;

        let mut password = String::new();
        std::io::stdin().read_line(&mut password)?;
        let password = password.trim();

        sess.userauth_password(user, password)?;
    }

    if !sess.authenticated() {
        return Err(eyre!("failed to authenticate ssh for {user}@{host}"));
    }

    let mut channel = sess.channel_session()?;

    channel.exec("lsof -i -P -n")?;
    let mut s = String::new();
    channel.read_to_string(&mut s)?;
    let ports = extract_port(&s);
    if ports.len() != 1 {
        return Err(eyre!("couldn't find specific port for ssh amongst: {ports:?}"));
    }
    let port = ports[0];

    // Close the channel and session
    channel.close()?;
    channel.wait_close()?;

    Ok(port)
}

#[instrument(level = "trace", skip_all)]
fn is_port_available(bind_addr: &str) -> bool {
    TcpListener::bind(bind_addr).is_ok()
}

#[instrument(level = "trace", skip_all)]
fn start_tunnel(local_port: u16, user: &str, host: &str, host_port: u16) -> Result<Child> {
    let child = Command::new("ssh")
        .args(["-L", &format!("{local_port}:localhost:{host_port}"), &format!("{user}@{host}"), "-f", "-N"])
        .spawn()?;
    Ok(child)
}

/// Store `pid`, keyed by `local_port`, for use by `kit disconnect`
#[instrument(level = "trace", skip_all)]
fn write_pid_to_file(local_port: u16, pid: u32) -> Result<()> {
    let kit_cache = std::path::PathBuf::from(KIT_CACHE);
    let connect_path = kit_cache.join("connect");
    if !connect_path.exists() {
        std::fs::create_dir_all(&connect_path)?;
    }
    std::fs::write(connect_path.join(format!("{}", local_port)), format!("{pid}"))?;
    Ok(())
}


#[instrument(level = "trace", skip_all)]
fn read_pid_from_file(local_port: u16) -> Result<u32> {
    let kit_cache = std::path::PathBuf::from(KIT_CACHE);
    let pid_file_path = kit_cache.join("connect").join(format!("{local_port}"));
    if !pid_file_path.exists() {
        return Err(eyre!("can't retrieve pid for local port {local_port}"));
    }
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
        return Err(eyre!("given local port {local_port} is not occupied: nothing to disconnect"));
    }

    let pid = read_pid_from_file(local_port)?;
    kill_pid(pid as i32)?;

    Ok(())
}

/// user & host required only for disconnect == false
#[instrument(level = "trace", skip_all)]
pub fn execute(
    local_port: u16,
    is_disconnect: bool,
    user: Option<&str>,
    host: Option<&str>,
    host_port: Option<u16>,
) -> Result<()> {
    if is_disconnect {
        // disconnect
        disconnect(local_port)?;
    } else {
        let Some(user) = user else {
            return Err(eyre!("user is a required field when connecting a new tunnel"));
        };
        let Some(host) = host else {
            return Err(eyre!("host is a required field when connecting a new tunnel"));
        };

        // connect: create connection
        if !is_port_available(&format!("127.0.0.1:{local_port}")) {
            return Err(eyre!("given local port {local_port} occupied")
                .with_suggestion(|| "try binding an open one")
            );
        }

        let (host_port, is_host_port_given) = match host_port {
            Some(hp) => (hp, true),
            None => (get_port(user, host)?, false),
        };

        let child = start_tunnel(local_port, user, host, host_port)?;

        let pid = child.id();
        write_pid_to_file(local_port, pid)?;
    }

    Ok(())
}
