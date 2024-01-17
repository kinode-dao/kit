use std::{fs, io, thread, time};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use zip::read::ZipArchive;

use tokio::sync::Mutex;

use super::build;
use super::run_tests::cleanup::{cleanup, cleanup_on_signal};
use super::run_tests::network_router;
use super::run_tests::types::*;

const KINODE_RELEASE_BASE_URL: &str = "https://github.com/uqbar-dao/kinode/releases/download";

fn extract_zip(archive_path: &Path) -> anyhow::Result<()> {
    let file = fs::File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;

    let archive_dir = archive_path.parent().unwrap_or_else(|| Path::new(""));

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };
        let outpath = archive_dir.join(outpath);

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(&p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    fs::remove_file(archive_path)?;

    Ok(())
}

pub fn compile_runtime(path: &Path, verbose: bool) -> anyhow::Result<()> {
    println!("Compiling Kinode runtime...");

    build::run_command(Command::new("cargo")
        .args(&["+nightly", "build", "--release", "--features", "simulation-mode"])
        .current_dir(path)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    println!("Done compiling Kinode runtime.");
    Ok(())
}

async fn get_runtime_binary_inner(
    version: &str,
    zip_name: &str,
    runtime_dir: &PathBuf,
) -> anyhow::Result<()> {
    let url = format!("{KINODE_RELEASE_BASE_URL}/{version}/{zip_name}");

    let runtime_zip_path = runtime_dir.join(zip_name);
    let runtime_path = runtime_dir.join("kinode");

    build::download_file(&url, &runtime_zip_path).await?;
    extract_zip(&runtime_zip_path)?;

    // Add execute permission
    let metadata = fs::metadata(&runtime_path)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    fs::set_permissions(&runtime_path, permissions)?;

    Ok(())
}

pub async fn get_runtime_binary(version: &str) -> anyhow::Result<PathBuf> {
    let uname = Command::new("uname").output()?;
    if !uname.status.success() {
        return Err(anyhow::anyhow!("Could not determine OS."));
    }
    let os_name = std::str::from_utf8(&uname.stdout)?.trim();

    let uname_p = Command::new("uname").arg("-p").output()?;
    if !uname_p.status.success() {
        return Err(anyhow::anyhow!("kit: Could not determine architecture."));
    }
    let architecture_name = std::str::from_utf8(&uname_p.stdout)?.trim();

    // TODO: update when have binaries
    let zip_name_midfix = match (os_name, architecture_name) {
        ("Linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("Darwin", "arm") => "aarch64-apple-darwin",
        ("Darwin", "i386") => "i386-apple-darwin",
        // ("Darwin", "x86_64") => "x86_64-apple-darwin",
        _ => return Err(anyhow::anyhow!("OS/Architecture {}/{} not supported.", os_name, architecture_name)),
    };
    let zip_name = format!("kinode-{}-simulation-mode.zip", zip_name_midfix);

    let runtime_dir = PathBuf::from(format!("/tmp/kinode-{}", version));
    let runtime_path = runtime_dir.join("kinode");

    if !runtime_dir.exists() {
        fs::create_dir_all(&runtime_dir)?;
        get_runtime_binary_inner(version, &zip_name, &runtime_dir).await?;
    }

    Ok(runtime_path)
}

pub fn run_runtime(
    path: &Path,
    home: &Path,
    port: u16,
    network_router_port: u16,
    args: &[&str],
    verbose: bool,
    detached: bool,
) -> anyhow::Result<(Child, OwnedFd)> {
    let port = format!("{}", port);
    let network_router_port = format!("{}", network_router_port);
    let mut full_args = vec![
        home.to_str().unwrap(), "--port", port.as_str(),
        "--network-router-port", network_router_port.as_str(),
    ];

    if !args.is_empty() {
        full_args.extend_from_slice(args);
    }

    let fds = nix::pty::openpty(None, None)?;

    let process = Command::new(path)
        .args(&full_args)
        .stdin(if !detached { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
        .spawn()?;

    Ok((process, fds.master))
}

pub async fn execute(
    runtime_path: Option<PathBuf>,
    version: String,
    node_home: PathBuf,
    node_port: u16,
    network_router_port: u16,
    rpc: Option<&str>,
    fake_node_name: &str,
    password: &str,
    is_persist: bool,
    mut args: Vec<&str>,
) -> anyhow::Result<()> {
    let detached = false;  // TODO: to argument?
    // TODO: factor out with run_tests?
    let runtime_path = match runtime_path {
        None => get_runtime_binary(&version).await?,
        Some(runtime_path) => {
            if !runtime_path.exists() {
                return Err(anyhow::anyhow!(
                    "--runtime-path {:?} does not exist.",
                    runtime_path,
                ));
            }
            if runtime_path.is_dir() {
                // Compile the runtime binary
                compile_runtime(&runtime_path, true)?;
                runtime_path.join("target/release/kinode")
            } else {
                return Err(anyhow::anyhow!(
                    "--runtime-path {:?} must be a directory (the repo).",
                    runtime_path,
                ));
            }
        },
    };

    let mut task_handles = Vec::new();

    let node_cleanup_infos = Arc::new(Mutex::new(Vec::new()));

    let (send_to_cleanup, recv_in_cleanup) = tokio::sync::mpsc::unbounded_channel();
    let (send_to_kill, _recv_kill) = tokio::sync::broadcast::channel(1);
    let recv_kill_in_cos = send_to_kill.subscribe();
    let recv_kill_in_router = send_to_kill.subscribe();

    let node_cleanup_infos_for_cleanup = Arc::clone(&node_cleanup_infos);
    let handle = tokio::spawn(cleanup(
        recv_in_cleanup,
        send_to_kill,
        node_cleanup_infos_for_cleanup,
        None,
        detached,
        !is_persist,
    ));
    task_handles.push(handle);
    let send_to_cleanup_for_signal = send_to_cleanup.clone();
    let handle = tokio::spawn(cleanup_on_signal(send_to_cleanup_for_signal, recv_kill_in_cos));
    task_handles.push(handle);
    let send_to_cleanup_for_cleanup = send_to_cleanup.clone();
    let _cleanup_context = CleanupContext::new(send_to_cleanup_for_cleanup);

    let network_router_port_for_router = network_router_port.clone();
    let handle = tokio::spawn(async move {
        let _ = network_router::execute(
            network_router_port_for_router,
            NetworkRouterDefects::None,
            recv_kill_in_router,
        ).await;
    });
    task_handles.push(handle);

    if node_home.exists() {
        fs::remove_dir_all(&node_home)?;
    }

    // TODO: can remove?
    thread::sleep(time::Duration::from_secs(1));

    if let Some(ref rpc) = rpc {
        args.extend_from_slice(&["--rpc", rpc]);
    };
    args.extend_from_slice(&["--fake-node-name", fake_node_name]);
    args.extend_from_slice(&["--password", password]);

    let (mut runtime_process, master_fd) = run_runtime(
        &runtime_path,
        &node_home,
        node_port,
        network_router_port,
        &args[..],
        true,
        detached,
    )?;

    let mut node_cleanup_infos = node_cleanup_infos.lock().await;
    node_cleanup_infos.push(NodeCleanupInfo {
        master_fd,
        process_id: runtime_process.id() as i32,
        home: node_home.clone(),
    });
    drop(node_cleanup_infos);

    runtime_process.wait().unwrap();
    let _ = send_to_cleanup.send(true);
    for handle in task_handles {
        handle.await.unwrap();
    }

    Ok(())
}
