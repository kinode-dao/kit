use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use zip::read::ZipArchive;

use color_eyre::{eyre::{eyre, Result, WrapErr}, Section};
use fs_err as fs;
use semver::Version;
use serde::Deserialize;
use tokio::process::{Child, Command as TCommand};
use tokio::sync::Mutex;
use tracing::{info, warn, instrument};

use crate::KIT_CACHE;
use crate::build;
use crate::chain;
use crate::run_tests::cleanup::{cleanup, cleanup_on_signal};
use crate::run_tests::types::*;

const KINODE_RELEASE_BASE_URL: &str = "https://github.com/kinode-dao/kinode/releases/download";
pub const KINODE_OWNER: &str = "kinode-dao";
const KINODE_REPO: &str = "kinode";
const LOCAL_PREFIX: &str = "/tmp/kinode-";
pub const CACHE_EXPIRY_SECONDS: u64 = 300;

#[derive(Deserialize, Debug)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize, Debug)]
struct Asset {
    name: String,
}

#[instrument(level = "trace", skip_all)]
fn extract_zip(archive_path: &Path) -> Result<()> {
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
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(&p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    fs::remove_file(archive_path)?;

    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub fn compile_runtime(path: &Path, release: bool) -> Result<()> {
    info!("Compiling Kinode runtime...");

    let mut args = vec![
        "+nightly",
        "build",
        "-p",
        "kinode",
        "--features",
        "simulation-mode",
        "--color=always",
    ];
    if release {
        args.push("--release");
    }

    build::run_command(Command::new("cargo")
        .args(&args)
        .current_dir(path),
        false,
    )?;

    info!("Done compiling Kinode runtime.");
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn get_runtime_binary_inner(
    version: &str,
    zip_name: &str,
    runtime_dir: &PathBuf,
) -> Result<()> {
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

#[instrument(level = "trace", skip_all)]
pub fn get_platform_runtime_name() -> Result<String> {
    let uname = Command::new("uname").output()?;
    if !uname.status.success() {
        return Err(eyre!("Could not determine OS."));
    }
    let os_name = std::str::from_utf8(&uname.stdout)?.trim();

    let uname_m = Command::new("uname").arg("-m").output()?;
    if !uname_m.status.success() {
        return Err(eyre!("Could not determine architecture."));
    }
    let architecture_name = std::str::from_utf8(&uname_m.stdout)?.trim();

    // TODO: update when have binaries
    let zip_name_midfix = match (os_name, architecture_name) {
        ("Linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("Darwin", "arm64") => "arm64-apple-darwin",
        ("Darwin", "x86_64") => "x86_64-apple-darwin",
        _ => {
            return Err(eyre!(
                "OS/Architecture {}/{} not amongst pre-built [Linux/x86_64, Apple/arm64, Apple/x86_64].",
                os_name,
                architecture_name,
            ).with_suggestion(|| "Use the `--runtime-path` flag to build a local copy of the https://github.com/kinode-dao/kinode repo")
            );
        }
    };
    Ok(format!("kinode-{}-simulation-mode.zip", zip_name_midfix))
}

#[instrument(level = "trace", skip_all)]
pub async fn get_runtime_binary(version: &str) -> Result<PathBuf> {
    let zip_name = get_platform_runtime_name()?;

    let version =
        if version != "latest" {
            version.to_string()
        } else {
            find_releases_with_asset_if_online(
                Some(KINODE_OWNER),
                Some(KINODE_REPO),
                &get_platform_runtime_name()?,
            )
            .await?
            .first()
            .ok_or_else(|| eyre!("No releases found"))?
            .clone()
        };

    let runtime_dir = PathBuf::from(format!("{}{}", LOCAL_PREFIX, version));
    let runtime_path = runtime_dir.join("kinode");

    if !runtime_dir.exists() {
        fs::create_dir_all(&runtime_dir)?;
        get_runtime_binary_inner(&version, &zip_name, &runtime_dir).await?;
    }

    Ok(runtime_path)
}

#[instrument(level = "trace", skip_all)]
pub async fn get_from_github(owner: &str, repo: &str, endpoint: &str) -> Result<Vec<u8>> {
    let cache_path = format!("{}/{}-{}-{}.bin", KIT_CACHE, owner, repo, endpoint);
    let cache_path = Path::new(&cache_path);
    if cache_path.exists() {
        if let Some(local_bytes) = fs::metadata(&cache_path).ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.elapsed().ok())
            .and_then(|since_modified| {
                if since_modified < Duration::from_secs(CACHE_EXPIRY_SECONDS) {
                    fs::read(&cache_path).ok()
                } else {
                    None
                }
            }) {
            return Ok(local_bytes);
        }
    }

    let url = format!("https://api.github.com/repos/{owner}/{repo}/{endpoint}");
    let client = reqwest::Client::new();
    match client.get(url)
        .header("User-Agent", "request")
        .send()
        .await?
        .bytes()
        .await {
        Ok(v) => {
            let v = v.to_vec();
            if let Ok(s) = String::from_utf8(v.clone()) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let serde_json::Value::String(ref s) = json["message"] {
                        if s.contains("API rate limit exceeded") {
                            warn!("GitHub throttled: can't fetch {owner}/{repo}/{endpoint}");
                            return Ok(vec![]);
                        }
                    }
                }
            }
            fs::create_dir_all(
                cache_path.parent().ok_or_else(|| eyre!("path doesn't have parent"))?
            )?;
            fs::write(&cache_path, &v)?;
            return Ok(v);
        },
        Err(_) => {
            warn!("GitHub throttled: can't fetch {owner}/{repo}/{endpoint}");
            return Ok(vec![]);
        },
    };
}

#[instrument(level = "trace", skip_all)]
async fn fetch_releases(owner: &str, repo: &str) -> Result<Vec<Release>> {
    let bytes = get_from_github(owner, repo, "releases").await?;
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[instrument(level = "trace", skip_all)]
pub async fn find_releases_with_asset(
    owner: Option<&str>,
    repo: Option<&str>,
    asset_name: &str,
) -> Result<Vec<String>> {
    let owner = owner.unwrap_or(KINODE_OWNER);
    let repo = repo.unwrap_or(KINODE_REPO);
    let releases = fetch_releases(owner, repo).await?;
    let filtered_releases: Vec<String> = releases.into_iter()
        .filter(|release| release.assets.iter().any(|asset| asset.name == asset_name))
        .map(|release| release.tag_name)
        .collect();
    Ok(filtered_releases)
}

pub async fn find_releases_with_asset_if_online(
    owner: Option<&str>,
    repo: Option<&str>,
    asset_name: &str,
) -> Result<Vec<String>> {
    let remote_values = match find_releases_with_asset(owner, repo, asset_name).await {
        Ok(v) => v,
        Err(e) => {
            match e.downcast_ref::<reqwest::Error>() {
                None => return Err(e),
                Some(ee) => {
                    if ee.is_connect() {
                        get_local_versions_with_prefix(&format!("{}v", LOCAL_PREFIX))?
                            .iter()
                            .map(|v| format!("v{}", v))
                            .collect()
                    } else {
                        return Err(e);
                    }
                },
            }
        },
    };
    Ok(remote_values)
}

#[instrument(level = "trace", skip_all)]
fn get_local_versions_with_prefix(prefix: &str) -> Result<Vec<String>> {
    let mut versions = Vec::new();

    for entry in fs::read_dir(Path::new(prefix).parent().unwrap())? {
        let entry = entry?;
        let path = entry.path();
        if let Some(str_path) = path.to_str() {
            if str_path.starts_with(prefix) {
                let version = str_path.replace(prefix, "");
                versions.push(version);
            }
        }
    }

    let mut sorted_versions: Vec<Version> = versions
        .into_iter()
        .filter_map(|s| Version::parse(&s).ok())
        .collect();
    sorted_versions.sort();

    let versions = sorted_versions
        .into_iter()
        .rev()
        .map(|v| v.to_string())
        .collect();

    Ok(versions)
}

#[instrument(level = "trace", skip_all)]
pub fn run_runtime(
    path: &Path,
    home: &Path,
    port: u16,
    fakechain_port: u16,
    name: &str,
    args: &[&str],
    verbose: bool,
    detached: bool,
    verbosity: u8,
) -> Result<(Child, OwnedFd)> {
    let port = format!("{}", port);
    let fakechain_port = format!("{}", fakechain_port);
    let verbosity = format!("{}", verbosity);
    let mut full_args = vec![
        home.to_str().unwrap(), "--port", port.as_str(),
        "--fake-node-name", name,
        "--fakechain-port", fakechain_port.as_str(),
        "--verbosity", verbosity.as_str(),
    ];

    if !args.is_empty() {
        full_args.extend_from_slice(args);
    }

    let fds = nix::pty::openpty(None, None)?;

    let process = TCommand::new(path)
        .args(&full_args)
        .stdin(if !detached { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
        .stdout(if verbose { Stdio::inherit() } else { Stdio::piped() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::piped() })
        .spawn()
        .wrap_err_with(|| format!("Couldn't open binary at path {:?}", path))?;

    Ok((process, fds.master))
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    runtime_path: Option<PathBuf>,
    version: String,
    node_home: PathBuf,
    node_port: u16,
    fakechain_port: u16,
    rpc: Option<&str>,
    mut fake_node_name: String,
    password: &str,
    is_persist: bool,
    release: bool,
    verbosity: u8,
    mut args: Vec<&str>,
) -> Result<()> {
    let detached = false;  // TODO: to argument?
    // TODO: factor out with run_tests?
    let runtime_path = match runtime_path {
        None => get_runtime_binary(&version).await?,
        Some(runtime_path) => {
            if !runtime_path.exists() {
                return Err(eyre!("--runtime-path {:?} does not exist.", runtime_path));
            }
            if runtime_path.is_dir() {
                // Compile the runtime binary
                compile_runtime(&runtime_path, release)?;
                runtime_path.join("target")
                    .join(if release { "release" } else { "debug" })
                    .join("kinode")
            } else {
                return Err(eyre!(
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
    let recv_kill_in_start_chain = send_to_kill.subscribe();

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

    // TODO: change this to be less restrictive; currently leads to weirdness
    //  like an input of `fake.os` -> `fake.os.dev`.
    //  The reason we need it for now is that non-`.dev` nodes are not currently
    //  addressable.
    //  Once they are addressable, change this to, perhaps, `!name.contains(".")
    if !fake_node_name.ends_with(".dev") {
        fake_node_name.push_str(".dev");
    }

    // boot fakechain
    let anvil_process = chain::start_chain(
        fakechain_port,
        true,
        recv_kill_in_start_chain,
        false,
    ).await?;

    if node_home.exists() {
        fs::remove_dir_all(&node_home)?;
    }

    if let Some(ref rpc) = rpc {
        args.extend_from_slice(&["--rpc", rpc]);
    };

    args.extend_from_slice(&["--password", password]);

    let (mut runtime_process, master_fd) = run_runtime(
        &runtime_path,
        &node_home,
        node_port,
        fakechain_port,
        &fake_node_name,
        &args[..],
        true,
        detached,
        verbosity,
    )?;

    let mut node_cleanup_infos = node_cleanup_infos.lock().await;
    node_cleanup_infos.push(NodeCleanupInfo {
        master_fd,
        process_id: runtime_process.id().unwrap() as i32,
        home: node_home.clone(),
        anvil_process: anvil_process.map(|ap| ap.id() as i32),
    });
    drop(node_cleanup_infos);

    runtime_process.wait().await.unwrap();
    let _ = send_to_cleanup.send(true);
    for handle in task_handles {
        handle.await.unwrap();
    }

    Ok(())
}
