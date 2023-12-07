use std::cell::RefCell;
use std::{fs, thread, time};
use std::os::fd::AsRawFd;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;

use dirs::home_dir;
use serde_json::Value;
use toml;

use super::boot_fake_node::run_runtime;
use super::build;
use super::inject_message;
use super::start_package;

pub mod network_router;
pub mod types;
use types::*;
mod tester_types;
use tester_types as tt;

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

fn make_node_names(nodes: Vec<Node>) -> anyhow::Result<Vec<String>> {
    nodes
        .iter()
        .map(|node| get_basename(&node.home)
            .and_then(|base| Some(base.to_string()))
            .and_then(|mut base| {
                if !base.ends_with(".uq") {
                    base.push_str(".uq");
                }
                Some(base)
            })
            .ok_or(anyhow::anyhow!(
                "run_tests:make_node_names: did not find basename for {:?}",
                node.home,
            ))
        )
        .collect()
}

impl Config {
    fn expand_home_paths(mut self: Config) -> Config {
        self.runtime_path = expand_home_path(&self.runtime_path).unwrap_or(self.runtime_path);
        for test in self.tests.iter_mut() {
            test.test_package_paths = test.test_package_paths
                .iter()
                .map(|tpp| expand_home_path(&tpp).unwrap_or_else(|| tpp.clone()))
                .collect();
            for node in test.nodes.iter_mut() {
                node.home = expand_home_path(&node.home).unwrap_or_else(|| node.home.clone());
            }
        }
        self
    }
}

fn compile_runtime(path: &Path, verbose: bool) -> anyhow::Result<()> {
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

// fn run_runtime(
//     path: &Path,
//     home: &str,
//     port: u16,
//     network_router_port: u16,
//     args: &[&str],
//     verbose: bool,
// ) -> anyhow::Result<(Child, OwnedFd)> {
//     let port = format!("{}", port);
//     let network_router_port = format!("{}", network_router_port);
//     let mut full_args = vec![
//         "+nightly", "run", "--release",
//         "--features", "simulation-mode", "--",
//         home, "--port", port.as_str(),
//         "--network-router-port", network_router_port.as_str(),
//     ];
//
//     if !args.is_empty() {
//         full_args.extend_from_slice(args);
//     }
//
//     let fds = nix::pty::openpty(None, None)?;
//
//     let process = Command::new("cargo")
//         .args(&full_args)
//         .current_dir(path)
//         .stdin(unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) })
//         .stdout(if verbose { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
//         .stderr(if verbose { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
//         .spawn()?;
//
//     Ok((process, fds.master))
// }

async fn wait_until_booted(port: u16, max_port_diff: u16, max_waits: u16) -> anyhow::Result<Option<u16>> {
    for _ in 0..max_waits {
        for port_scan in port..port + max_port_diff {
            let request = inject_message::make_message(
                "vfs:sys:uqbar",
                &serde_json::to_string(&serde_json::json!({
                    "drive": "tester:uqbar",
                    "action": {"GetEntry": "/"},
                })).unwrap(),
                None,
                None,
                None,
            )?;

            match inject_message::send_request(
                &format!("http://localhost:{}", port_scan),
                request,
            ).await {
                Ok(response) if response.status() == 200 => return Ok(Some(port_scan)),
                _ => ()
            }

            thread::sleep(time::Duration::from_millis(100));
        }
        thread::sleep(time::Duration::from_secs(1));
    }
    println!("Failed to find Uqbar on ports {}-{}", port, port + max_port_diff);
    Ok(None)
}

async fn load_setups(setup_paths: &Vec<PathBuf>, port: u16) -> anyhow::Result<()> {
    println!("Loading setup packages...");

    for setup_path in setup_paths {
        start_package::execute(
            setup_path.clone(),
            &format!("http://localhost:{}", port),
            None,
        ).await?;
    }

    println!("Done loading setup packages.");
    Ok(())
}

async fn load_tests(test_paths: &Vec<PathBuf>, port: u16) -> anyhow::Result<()> {
    println!("Loading tests...");
    for test_path in test_paths {
        let basename = get_basename(&test_path).unwrap();
        let request = inject_message::make_message(
            "vfs:sys:uqbar",
            &serde_json::to_string(&serde_json::json!({
                "drive": "tester:uqbar",
                "action": {
                    "Add": {
                        "full_path": format!("/{basename}.wasm"),
                        "entry_type": "NewFile",
                    }
                }
            })).unwrap(),
            None,
            None,
            test_path.join("pkg").join(format!("{basename}.wasm")).to_str(),
        )?;

        match inject_message::send_request(
            &format!("http://localhost:{}", port),
            request,
        ).await {
            Ok(response) if response.status() != 200 => println!("Failed with status code: {}", response.status()),
            _ => ()
        }
    }
    println!("Done loading tests.");
    Ok(())
}

async fn run_tests(test_batch: &str, mut ports: Vec<u16>, node_names: Vec<String>) -> anyhow::Result<()> {
    let master_port = ports.remove(0);

    // Set up non-master nodes.
    for port in ports {
        let request = inject_message::make_message(
            "tester:tester:uqbar",
            &serde_json::to_string(&serde_json::json!({"Run": node_names})).unwrap(),
            // "{\"Run\":null}",
            None,
            None,
            None,
        )?;
        let response = inject_message::send_request(
            &format!("http://localhost:{}", port),
            request,
        ).await?;

        if response.status() != 200 {
            println!("Failed with status code: {}", response.status());
            return Err(anyhow::anyhow!("Failed with status code: {}", response.status()))
        }
    }

    // Set up master node & start tests.
    println!("Running tests...");
    let request = inject_message::make_message(
        "tester:tester:uqbar",
        &serde_json::to_string(&serde_json::json!({"Run": node_names})).unwrap(),
        // "{\"Run\":null}",
        None,
        None,
        None,
    )?;
    let response = inject_message::send_request(
        &format!("http://localhost:{}", master_port),
        request,
    ).await?;

    if response.status() != 200 {
        println!("Failed with status code: {}", response.status());
        return Err(anyhow::anyhow!("Failed with status code: {}", response.status()))
    } else {
        let content: String = response.text().await?;

        let mut data: Option<Value> = serde_json::from_str(&content).ok();

        println!("Done running tests ({}):", test_batch);

        if let Some(ref mut data_map) = data {
            if let Some(serde_json::Value::Array(ipc_bytes_val)) = data_map.get("ipc") {
                let ipc_bytes: Vec<u8> = ipc_bytes_val.iter().map(|n| n.as_u64().unwrap() as u8).collect();
                let ipc_string: String = String::from_utf8(ipc_bytes)?;
                match serde_json::from_str(&ipc_string)? {
                    tt::TesterResponse::Pass => println!("PASS"),
                    tt::TesterResponse::Fail { test, file, line, column } => {
                        let s = format!("FAIL: {} {}:{}:{}", test, file, line, column);
                        println!("{}", s);
                        return Err(anyhow::anyhow!(s));
                    },
                    tt::TesterResponse::GetFullMessage(_) => {
                        let s = "FAIL: Unexpected Response";
                        println!("{}", s);
                        return Err(anyhow::anyhow!(s))
                    },
                }
            } else {
                println!("Test FAIL: unexpected Response: {:?}", data);
            }
        }
    }

    Ok(())
}

pub async fn execute(config_path: &str) -> anyhow::Result<()> {
    let config_content = fs::read_to_string(config_path)?;
    let mut config = toml::from_str::<Config>(&config_content)?.expand_home_paths();

    // println!("{:?}", config);

    if config.runtime_path.is_dir() {
        // Compile the runtime binary
        compile_runtime(
            &config.runtime_path,
            config.runtime_build_verbose,
        )?;
        config.runtime_path = config.runtime_path.join("/target/release/uqbar");
    } else if config.runtime_path.is_file() {
        // Pass
    } else {
        panic!("rt"); // TODO
    }


    for test in config.tests {
        for setup_package_path in &test.setup_package_paths {
            build::compile_package(&setup_package_path, test.package_build_verbose).await?;
        }
        for test_package_path in &test.test_package_paths {
            build::compile_package(&test_package_path, test.package_build_verbose).await?;
        }

        // Initialize variables for master node and nodes list
        let mut master_node_port = None;
        let nodes = Rc::new(RefCell::new(Vec::new()));

        // Cleanup, boot check, test loading, and running
        let (send_to_kill_router, recv_kill_in_router) = tokio::sync::mpsc::unbounded_channel();
        let _cleanup_context = CleanupContext::new(Rc::clone(&nodes), send_to_kill_router);

        // Process each node
        for node in &test.nodes {
            fs::create_dir_all(&node.home)?;
            let node_home = fs::canonicalize(&node.home)?;
            let home_fs = Path::new(node_home.to_str().unwrap())
                .join("fs");
            if home_fs.exists() {
                fs::remove_dir_all(home_fs).unwrap();
            }

            let mut args = vec![];
            if let Some(ref rpc) = node.rpc {
                args.extend_from_slice(&["--rpc", rpc]);
            };
            if let Some(ref fake_node_name) = node.fake_node_name {
                args.extend_from_slice(&["--fake-node-name", fake_node_name]);
            };
            if let Some(ref password) = node.password {
                args.extend_from_slice(&["--password", password]);
            };

            let (runtime_process, master_fd) = run_runtime(
                &config.runtime_path,
                &node_home,
                node.port,
                test.network_router.port,
                &args[..],
                node.runtime_verbose,
                true,
            )?;

            let node_info = NodeInfo {
                process_handle: runtime_process,
                master_fd,
                port: node.port,
                home: node_home.clone(),
            };

            if master_node_port.is_none() {
                master_node_port = Some(node_info.port.clone());
            }
            nodes.borrow_mut().push(node_info);
        }

        tokio::task::spawn(network_router::execute(
            test.network_router.port.clone(),
            test.network_router.defects.clone(),
            recv_kill_in_router,
        ));

        let mut ports = Vec::new();

        // Cleanup, boot check, test loading, and running
        for node in nodes.borrow_mut().iter_mut() {
            let node_home = fs::canonicalize(&node.home)?;
            println!("Setting up node {:?}...", node_home);
            node.port = wait_until_booted(node.port, 5, 5).await?.unwrap();
            ports.push(node.port);
            println!("Done setting up node {:?} on port {}.", node_home, node.port);
        }

        for port in &ports {
            load_setups(&test.setup_package_paths, port.clone()).await?;
        }
        load_tests(&test.test_package_paths, master_node_port.unwrap().clone()).await?;

        run_tests(
            &format!("{:?}", test.test_package_paths),
            ports,
            make_node_names(test.nodes)?,
        ).await?;

        //let _ = send_to_kill_router.send(true).await;
    }

    Ok(())
}
