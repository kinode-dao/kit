use std::io::Read;
use std::cell::RefCell;
use std::{fs, io, thread, time};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use zip::read::ZipArchive;

use super::build;
use super::run_tests::network_router;
use super::run_tests::types::*;

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

    Ok(())
}

pub fn compile_runtime(path: &Path, verbose: bool) -> anyhow::Result<()> {
    println!("Compiling Uqbar runtime...");

    build::run_command(Command::new("cargo")
        .args(&["+nightly", "build", "--release", "--features", "simulation-mode"])
        .current_dir(path)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    println!("Done compiling Uqbar runtime.");
    Ok(())
}

pub async fn get_runtime_binary(version: &str) -> anyhow::Result<PathBuf> {
    let uname = Command::new("uname").output()?;
    if !uname.status.success() {
        panic!("uqdev: Could not determine OS.");
    }
    let os_name = std::str::from_utf8(&uname.stdout)?.trim();

    let uname_p = Command::new("uname").arg("-p").output()?;
    if !uname_p.status.success() {
        panic!("uqdev: Could not determine architecture.");
    }
    let architecture_name = std::str::from_utf8(&uname_p.stdout)?.trim();

    // TODO: update when have binaries
    let binary_suffix = match (os_name, architecture_name) {
        ("Linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("Darwin", "arm") => "arm-apple-darwin",
        ("Darwin", "i386") => "i386-apple-darwin",
        // ("Darwin", "x86_64") => "x86_64-apple-darwin",
        _ => panic!("OS/Architecture {}/{} not supported.", os_name, architecture_name),
    };

    let binary = format!("uqbar-{}", binary_suffix);
    let url = format!("https://github.com/uqbar-dao/uqbin/raw/master/{version}/{binary}.zip");

    let runtime_dir = PathBuf::from(format!("/tmp/uqbar-{}", version));
    let runtime_zip_path = runtime_dir.join(format!("{}.zip", binary));
    let runtime_path = runtime_dir.join(binary).join("uqbar");
    if !runtime_path.exists() {
        fs::create_dir_all(&runtime_dir)?;
        build::download_file(&url, &runtime_zip_path).await?;
        extract_zip(&runtime_zip_path)?;

        // Add execute permission
        let metadata = fs::metadata(&runtime_path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        fs::set_permissions(&runtime_path, permissions)?;
    }

    Ok(runtime_path)
}

fn files_are_same(file_path1: PathBuf, file_path2: PathBuf) -> anyhow::Result<bool> {
    let mut file1 = fs::File::open(file_path1)?;
    let mut file2 = fs::File::open(file_path2)?;

    let mut buffer1 = Vec::new();
    let mut buffer2 = Vec::new();

    file1.read_to_end(&mut buffer1)?;
    file2.read_to_end(&mut buffer2)?;

    let diff = similar::capture_diff_slices(similar::Algorithm::Myers, &buffer1, &buffer2);

    Ok(diff.iter().all(|op| matches!(op, similar::DiffOp::Equal { .. })))
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
        .current_dir(path.parent().unwrap())
        .stdin(if !detached { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
        .stdout(if verbose { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
        .stderr(if verbose { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
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
    mut args: Vec<&str>,
) -> anyhow::Result<()> {
    // TODO: factor out with run_tests?
    let runtime_path = match runtime_path {
        None => get_runtime_binary(&version).await?,
        Some(runtime_path) => {
            if !runtime_path.exists() {
                panic!("uqdev boot-fake-node: RepoPath {:?} does not exist.", runtime_path);
            }
            if runtime_path.is_file() {
                // TODO: make loading/finding base processes more robust
                panic!("uqdev boot-fake-node: path to binary not yet implemented; please pass path to Uqbar core repo (or use --version)")
                // runtime_path
            } else if runtime_path.is_dir() {
                // Compile the runtime binary
                compile_runtime(&runtime_path, true)?;
                if !files_are_same(
                    runtime_path.join("target/release/uqbar"),
                    runtime_path.join("uqbar"),
                )? {
                    fs::copy(
                        runtime_path.join("target/release/uqbar"),
                        runtime_path.join("uqbar"),
                    )?;
                }
                runtime_path.join("uqbar")
            } else {
                panic!("uqdev boot-fake-node: --runtime-path {:?} must be a directory (the repo) or a binary.", runtime_path);
            }
        },
    };

    let (send_to_kill_router, recv_kill_in_router) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn(network_router::execute(
        network_router_port.clone(),
        NetworkRouterDefects::None,
        recv_kill_in_router,
    ));

    thread::sleep(time::Duration::from_secs(1));

    let nodes = Rc::new(RefCell::new(Vec::new()));
    let _cleanup_context = CleanupContext::new(Rc::clone(&nodes), send_to_kill_router);

    if let Some(ref rpc) = rpc {
        args.extend_from_slice(&["--rpc", rpc]);
    };
    args.extend_from_slice(&["--fake-node-name", fake_node_name]);
    args.extend_from_slice(&["--password", password]);

    let (runtime_process, master_fd) = run_runtime(
        &runtime_path,
        &node_home,
        node_port,
        network_router_port,
        &args[..],
        true,
        false,
    )?;

    let node_info = NodeInfo {
        process_handle: runtime_process,
        master_fd,
        port: node_port,
        home: node_home.clone(),
    };

    nodes.borrow_mut().push(node_info);

    nodes.borrow_mut()[0].process_handle.wait().unwrap();

    Ok(())
}
