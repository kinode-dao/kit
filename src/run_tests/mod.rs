use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::{eyre::eyre, Result};
use dirs::home_dir;
use fs_err as fs;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, instrument};

use crate::boot_fake_node::{compile_runtime, get_runtime_binary, run_runtime};
use crate::build;
use crate::chain;
use crate::inject_message;
use crate::start_package;

use crate::kinode::process::tester::{Response as TesterResponse, FailResponse};

pub mod cleanup;
use cleanup::{cleanup, cleanup_on_signal, drain_print_runtime};
pub mod types;
use types::*;

fn get_basename(file_path: &Path) -> Option<&str> {
    file_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
}

fn expand_home_path_string(path: &str) -> Option<String> {
    if path.starts_with("~/") {
        if let Some(home_path) = home_dir() {
            return Some(path.replacen("~", &home_path.to_string_lossy(), 1));
        }
    }
    None
}

fn expand_home_path(path: &PathBuf) -> Option<PathBuf> {
    path.as_os_str()
        .to_str()
        .and_then(|s| expand_home_path_string(s))
        .and_then(|s| Some(Path::new(&s).to_path_buf()))
}

#[instrument(level = "trace", skip_all)]
fn make_node_names(nodes: Vec<Node>) -> Result<Vec<String>> {
    nodes
        .iter()
        .map(|node| get_basename(&node.home)
            .and_then(|base| Some(base.to_string()))
            .and_then(|mut base| {
                if !base.ends_with(".dev") {
                    base.push_str(".dev");
                }
                Some(base)
            })
            .ok_or_else(|| {
                eyre!("run_tests:make_node_names: did not find basename for {:?}", node.home)
            })
        )
        .collect()
}

impl Config {
    fn expand_home_paths(mut self: Config) -> Config {
        self.runtime = match self.runtime {
            Runtime::FetchVersion(version) => Runtime::FetchVersion(version),
            Runtime::RepoPath(runtime_path) => {
                Runtime::RepoPath(expand_home_path(&runtime_path).unwrap_or(runtime_path))
            },
        };
        for test in self.tests.iter_mut() {
            test.test_packages = test.test_packages
                .iter()
                .map(|tp| {
                    TestPackage {
                        path: expand_home_path(&tp.path).unwrap_or_else(|| tp.path.clone()),
                        grant_capabilities: tp.grant_capabilities.clone(),
                    }
                })
                .collect();
            for node in test.nodes.iter_mut() {
                node.home = expand_home_path(&node.home).unwrap_or_else(|| node.home.clone());
            }
        }
        self
    }
}

#[instrument(level = "trace", skip_all)]
async fn wait_until_booted(
    node: &PathBuf,
    port: u16,
    max_waits: u16,
    mut recv_kill_in_wait: BroadcastRecvBool,
) -> Result<()> {
    info!("Waiting for node {:?} on port {} to be ready...", node, port);
    for _ in 0..max_waits {
        let request = inject_message::make_message(
            "vfs:distro:sys",
            Some(15),
            &serde_json::to_string(&serde_json::json!({
                "path": "/tester:sys/pkg",
                "action": "ReadDir",
            })).unwrap(),
            None,
            None,
            None,
        )?;

        match inject_message::send_request_inner(
            &format!("http://localhost:{}", port),
            request,
        ).await {
            Ok(response) => match inject_message::parse_response(response).await {
                Ok(_) => {
                    info!("Done waiting for node {:?} on port {}.", node, port);
                    return Ok(())
                },
                _ => (),
            }
            _ => (),
        }

        tokio::select! {
            _ = sleep(Duration::from_secs(1)) => {}
            _ = recv_kill_in_wait.recv() => {
                return Err(eyre!("received exit"));
            }
        };
    }
    Err(eyre!("kit run-tests: could not connect to Kinode"))
}

#[instrument(level = "trace", skip_all)]
async fn load_setups(setup_paths: &Vec<PathBuf>, port: u16) -> Result<()> {
    info!("Loading setup packages...");

    for setup_path in setup_paths {
        start_package::execute(&setup_path, &format!("http://localhost:{}", port)).await?;
    }

    info!("Done loading setup packages.");
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn load_tests(test_packages: &Vec<TestPackage>, port: u16) -> Result<()> {
    info!("Loading tests...");

    for TestPackage { ref path, .. } in test_packages {
        let basename = get_basename(path).unwrap();
        let request = inject_message::make_message(
            "vfs:distro:sys",
            Some(15),
            &serde_json::to_string(&serde_json::json!({
                "path": format!("/tester:sys/tests/{basename}.wasm"),
                "action": "Write",
            })).unwrap(),
            None,
            None,
            path.join("pkg").join(format!("{basename}.wasm")).to_str(),
        )?;

        let response = inject_message::send_request(
            &format!("http://localhost:{}", port),
            request,
        ).await?;
        match inject_message::parse_response(response).await {
            Ok(_) => {},
            Err(e) => return Err(eyre!("Failed to load tests: {}", e)),
        }
    }

    let mut grant_caps = std::collections::HashMap::new();
    for TestPackage { ref path, ref grant_capabilities } in test_packages {
        grant_caps.insert(
            path.file_name().map(|f| f.to_str()).unwrap(),
            grant_capabilities,
        );
    }
    let grant_caps = serde_json::to_vec(&grant_caps)?;

    let request = inject_message::make_message(
        "vfs:distro:sys",
        Some(15),
        &serde_json::to_string(&serde_json::json!({
            "path": format!("/tester:sys/tests/grant_capabilities.json"),
            "action": "Write",
        })).unwrap(),
        None,
        Some(&grant_caps),
        None,
    )?;

    let response = inject_message::send_request(
        &format!("http://localhost:{}", port),
        request,
    ).await?;
    match inject_message::parse_response(response).await {
        Ok(_) => {},
        Err(e) => return Err(eyre!("Failed to load tests capabilities: {}", e)),
    }


    info!("Done loading tests.");
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn run_tests(
    test_packages: &Vec<TestPackage>,
    mut ports: Vec<u16>,
    node_names: Vec<String>,
    test_timeout: u64,
) -> Result<()> {
    let master_port = ports.remove(0);

    // Set up non-master nodes.
    for port in ports {
        let request = inject_message::make_message(
            "tester:tester:sys",
            Some(15),
            &serde_json::to_string(&serde_json::json!({
                "Run": {
                    "input_node_names": node_names,
                    "test_names": test_packages
                        .iter()
                        .map(|tp| tp.path.to_str().unwrap())
                        .collect::<Vec<&str>>(),
                    "test_timeout": test_timeout,
                }
            })).unwrap(),
            None,
            None,
            None,
        )?;
        let response = inject_message::send_request(
            &format!("http://localhost:{}", port),
            request,
        ).await?;

        if response.status() != 200 {
            return Err(eyre!("Failed with status code: {}", response.status()))
        }
    }

    // Set up master node & start tests.
    info!("Running tests...");
    let request = inject_message::make_message(
        "tester:tester:sys",
        Some(15),
        &serde_json::to_string(&serde_json::json!({
            "Run": {
                "input_node_names": node_names,
                "test_names": test_packages
                    .iter()
                    .map(|tp| tp.path.to_str().unwrap())
                    .collect::<Vec<&str>>(),
                "test_timeout": test_timeout,
            }
        })).unwrap(),
        None,
        None,
        None,
    )?;
    let response = inject_message::send_request(
        &format!("http://localhost:{}", master_port),
        request,
    ).await?;

    match inject_message::parse_response(response).await {
        Ok(inject_message::Response { ref body, .. }) => {
            let TesterResponse::Run(result) = serde_json::from_str(body)?;
            match result {
                Ok(()) => info!("PASS"),
                Err(FailResponse { test, file, line, column }) => {
                    return Err(eyre!("FAIL: {} {}:{}:{}", test, file, line, column));
                }
            }
        },
        Err(e) => {
            return Err(eyre!("FAIL: {}", e));
        },
    };

    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn handle_test(
    detached: bool,
    runtime_path: &Path,
    test: Test,
    test_dir_path: &Path,
) -> Result<()> {
    let setup_package_paths: Vec<PathBuf> = test.setup_package_paths
        .iter()
        .cloned()
        .map(|p| test_dir_path.join(p).canonicalize().unwrap())
        .collect();
    let test_packages: Vec<TestPackage> = test.test_packages
        .iter()
        .cloned()
        .map(|tp| {
            TestPackage {
                path: test_dir_path.join(tp.path).canonicalize().unwrap(),
                grant_capabilities: tp.grant_capabilities,
            }
        })
        .collect();
    //for setup_package_path in &test.setup_package_paths {
    //    let path = test_dir_path.join(setup_package_path).canonicalize()?;
    for path in &setup_package_paths {
        build::execute(&path, false, false, false, "", None, None, false).await?;  // TODO
    }
    for TestPackage { ref path, .. } in &test_packages {
        let path = test_dir_path.join(path).canonicalize()?;
        build::execute(&path, false, false, false, "", None, None, false).await?;  // TODO
    }

    // Initialize variables for master node and nodes list
    let mut master_node_port = None;
    let mut task_handles = Vec::new();
    let node_handles = Arc::new(Mutex::new(Vec::new()));
    let node_cleanup_infos = Arc::new(Mutex::new(Vec::new()));

    // Cleanup, boot check, test loading, and running
    let (send_to_cleanup, recv_in_cleanup) = tokio::sync::mpsc::unbounded_channel();
    let (send_to_kill, _recv_kill) = tokio::sync::broadcast::channel(1);
    let recv_kill_in_cos = send_to_kill.subscribe();
    let node_cleanup_infos_for_cleanup = Arc::clone(&node_cleanup_infos);
    let node_handles_for_cleanup = Arc::clone(&node_handles);
    let send_to_kill_for_cleanup = send_to_kill.clone();
    let handle = tokio::spawn(cleanup(
        recv_in_cleanup,
        send_to_kill_for_cleanup,
        node_cleanup_infos_for_cleanup,
        Some(node_handles_for_cleanup),
        detached,
        true,
    ));
    task_handles.push(handle);
    let send_to_cleanup_for_signal = send_to_cleanup.clone();
    let handle = tokio::spawn(cleanup_on_signal(send_to_cleanup_for_signal, recv_kill_in_cos));
    task_handles.push(handle);
    let _cleanup_context = CleanupContext::new(send_to_cleanup.clone());

    // boot fakechain
    let recv_kill_in_start_chain = send_to_kill.subscribe();
    let anvil_process = chain::start_chain(
        test.fakechain_router,
        true,
        recv_kill_in_start_chain,
        false,
    ).await?;

    // Process each node
    for node in &test.nodes {
        fs::create_dir_all(&node.home)?;
        let node_home = fs::canonicalize(&node.home)?;
        for dir in &["kernel", "kv", "sqlite", "vfs"] {
            let dir = node_home.join(dir);
            if dir.exists() {
                fs::remove_dir_all(&node_home.join(dir)).unwrap();
            }
        }

        let mut args = vec![];
        if let Some(ref rpc) = node.rpc {
            args.extend_from_slice(&["--rpc".into(), rpc.clone()]);
        };
        if let Some(ref password) = node.password {
            args.extend_from_slice(&["--password".into(), password.clone()]);
        };

        // TODO: change this to be less restrictive; currently leads to weirdness
        //  like an input of `fake.os` -> `fake.os.dev`.
        //  The reason we need it for now is that non-`.dev` nodes are not currently
        //  addressable.
        //  Once they are addressable, change this to, perhaps, `!name.contains(".")
        let mut name = node.fake_node_name.clone();
        if !name.ends_with(".dev") {
            name.push_str(".dev");
        }

        args.extend_from_slice(&[
            "--fake-node-name".into(), name,
            "--fakechain-port".into(), format!("{}", test.fakechain_router),
        ]);

        let (mut runtime_process, master_fd) = run_runtime(
            &runtime_path,
            &node_home,
            node.port,
            &args[..],
            false,
            detached,
            node.runtime_verbosity.unwrap_or_else(|| 0u8),
        )?;

        let mut anvil_cleanup: Option<i32> = None;

        if master_node_port.is_none() {
            anvil_cleanup = anvil_process.as_ref().map(|ap| ap.id() as i32);
            master_node_port = Some(node.port);
        };

        let mut node_cleanup_infos = node_cleanup_infos.lock().await;
        node_cleanup_infos.push(NodeCleanupInfo {
            master_fd,
            process_id: runtime_process.id().unwrap() as i32,
            home: node_home.clone(),
            anvil_process: anvil_cleanup,
        });

        let recv_kill_in_dpr = send_to_kill.subscribe();
        tokio::spawn(drain_print_runtime(
            runtime_process.stdout.take().unwrap(),
            runtime_process.stderr.take().unwrap(),
            recv_kill_in_dpr,
        ));

        let mut node_handles = node_handles.lock().await;
        node_handles.push(runtime_process);

        let recv_kill_in_wait = send_to_kill.subscribe();
        wait_until_booted(&node.home, node.port, 10, recv_kill_in_wait).await?;
    }

    for node in &test.nodes {
        load_setups(&setup_package_paths, node.port.clone()).await?;
    }

    load_tests(&test_packages, master_node_port.unwrap().clone()).await?;

    let ports = test.nodes.iter().map(|n| n.port).collect();

    let tests_result = run_tests(
        &test.test_packages,
        ports,
        make_node_names(test.nodes)?,
        test.timeout_secs,
    ).await;

    let _ = send_to_cleanup.send(tests_result.is_err());
    for handle in task_handles {
        handle.await.unwrap();
    }

    tests_result?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(config_path: &str) -> Result<()> {
    let detached = true; // TODO: to arg?

    let config_path = PathBuf::from(config_path);
    if !config_path.exists() {
        return Err(eyre!("given config path {config_path:?} does not exist"));
    }
    let config_path = if config_path.is_file() {
        config_path
    } else {
        let config_path = config_path.join("test").join("tests.toml");
        if !config_path.exists() {
            return Err(eyre!("given config path {config_path:?} does not exist"));
        }
        if config_path.is_file() {
            config_path
        } else {
            return Err(eyre!("given config path {config_path:?} is not a file"));
        }
    };

    let content = fs::read_to_string(&config_path)?;
    let config = toml::from_str::<Config>(&content)?.expand_home_paths();

    debug!("{:?}", config);

    // TODO: factor out with boot_fake_node?
    let runtime_path = match config.runtime {
        Runtime::FetchVersion(ref version) => get_runtime_binary(version, true).await?,
        Runtime::RepoPath(runtime_path) => {
            if !runtime_path.exists() {
                return Err(eyre!("RepoPath {:?} does not exist.", runtime_path));
            }
            if runtime_path.is_dir() {
                // Compile the runtime binary
                compile_runtime(
                    &runtime_path,
                    config.runtime_build_release,
                    true,
                )?;
                runtime_path.join("target")
                    .join(if config.runtime_build_release { "release" } else { "debug" })
                    .join("kinode")
            } else {
                return Err(eyre!(
                    "RepoPath {:?} must be a directory (the repo).",
                    runtime_path
                ));
            }
        },
    };

    let test_dir_path = PathBuf::from(config_path).canonicalize()?;
    let test_dir_path = test_dir_path.parent().unwrap();
    for test in config.tests {
        handle_test(detached, &runtime_path, test, &test_dir_path).await?;
    }

    Ok(())
}
