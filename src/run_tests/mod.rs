use std::cell::RefCell;
use std::error::Error;
use std::{fs, thread, time};
use std::os::fd::AsRawFd;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;

use dirs::home_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml;

use super::build;
use super::inject_message;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    runtime_path: PathBuf,
    is_runtime_print: bool,
    tests: Vec<Test>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Test {
    test_process_paths: Vec<PathBuf>,
    nodes: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Node {
    port: u16,
    home: PathBuf,
    password: String,
    rpc: String,
    is_runtime_print: bool,
}

#[derive(Debug)]
struct NodeInfo {
    process_handle: Child,
    master_fd: OwnedFd,
    port: u16,
    home: PathBuf,
}

struct CleanupContext {
    nodes: Rc<RefCell<Vec<NodeInfo>>>,
}

impl CleanupContext {
    fn new(nodes: Rc<RefCell<Vec<NodeInfo>>>) -> Self {
        CleanupContext { nodes }
    }
}

impl Drop for CleanupContext {
    fn drop(&mut self) {
        for node in self.nodes.borrow_mut().iter_mut() {
            cleanup_node(node);
        }
    }
}

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

impl Config {
    fn expand_home_paths(mut self: Config) -> Config {
        self.runtime_path = expand_home_path(&self.runtime_path).unwrap_or(self.runtime_path);
        for test in self.tests.iter_mut() {
            test.test_process_paths = test.test_process_paths
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

fn cleanup_node(node: &mut NodeInfo) {
    // Assuming Node is a struct that contains process_handle, master_fd, and home
    // Send Ctrl-C to the process
    nix::unistd::write(node.master_fd.as_raw_fd(), b"\x03").unwrap();
    node.process_handle.wait().unwrap();

    let home_fs = Path::new(&node.home).join("fs");
    if home_fs.exists() {
        fs::remove_dir_all(home_fs).unwrap();
    }
}

fn compile_runtime(path: &Path, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Compiling Uqbar runtime...");

    build::run_command(Command::new("cargo")
        .args(&["+nightly", "build", "--release", "--features", "simulation-mode"])
        .current_dir(path)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    Ok(())
}

fn run_runtime(
    path: &Path,
    home: &str,
    port: u16,
    args: &[&str],
    verbose: bool,
) -> Result<(Child, OwnedFd), Box<dyn std::error::Error>> {
    let port = format!("{}", port);
    let mut full_args = vec![
        "+nightly", "run", "--release",
        "--features", "simulation-mode", "--",
        "--port", port.as_str(), "--home", home,
    ];

    if !args.is_empty() {
        full_args.extend_from_slice(args);
    }

    let fds = nix::pty::openpty(None, None)?;

    let process = Command::new("cargo")
        .args(&full_args)
        .current_dir(path)
        .stdin(unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) })
        .stdout(if verbose { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
        .stderr(if verbose { Stdio::inherit() } else { unsafe { Stdio::from_raw_fd(fds.slave.as_raw_fd()) } })
        .spawn()?;

    Ok((process, fds.master))
}

async fn wait_until_booted(port: u16, max_port_diff: u16, max_waits: u16) -> Result<Option<u16>, Box<dyn Error>> {
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

async fn load_tests(test_paths: &Vec<PathBuf>, port: u16) -> Result<(), Box<dyn Error>> {
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
    Ok(())
}

async fn run_tests(test_batch: &str, port: u16) -> Result<(), Box<dyn Error>> {
    let request = inject_message::make_message(
        "tester:tester:uqbar",
        "{\"Run\":null}",
        None,
        None,
        None,
    )?;
    let response = inject_message::send_request(
        &format!("http://localhost:{}", port),
        request,
    ).await?;

    if response.status() == 200 {
        let content: String = response.text().await?;
        let mut data: Option<Value> = serde_json::from_str(&content).ok();

        if let Some(ref mut data_map) = data {
            if let Some(ipc_str) = data_map["ipc"].as_str() {
                let ipc_json: Value = serde_json::from_str(ipc_str).unwrap_or(Value::Null);
                data_map["ipc"] = ipc_json;
            }

            if let Some(payload_str) = data_map["payload"].as_str() {
                let payload_json: Value = serde_json::from_str(payload_str).unwrap_or(Value::Null);
                data_map["bytes"] = payload_json;
            }
        }
        println!("{} {:?}", test_batch, content);
    } else {
        println!("Failed with status code: {}", response.status());
    }

    Ok(())
}

pub async fn execute(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = fs::read_to_string(config_path)?;
    let config = toml::from_str::<Config>(&config_content)?.expand_home_paths();

    println!("{:?}", config);

    // Compile the runtime binary
    compile_runtime(Path::new(config.runtime_path.to_str().unwrap()), config.is_runtime_print)?;

    for test in config.tests {
        for test_process_path in &test.test_process_paths {
            build::compile_process(&test_process_path)?;
        }

        // Initialize variables for master node and nodes list
        let mut master_node_port = None;
        let nodes = Rc::new(RefCell::new(Vec::new()));

        // Cleanup, boot check, test loading, and running
        let _cleanup_context = CleanupContext::new(Rc::clone(&nodes));

        // Process each node
        for node in test.nodes {
            let home_fs = Path::new(node.home.to_str().unwrap())
                .join("fs");
            if home_fs.exists() {
                fs::remove_dir_all(home_fs).unwrap();
            }

            let (runtime_process, master_fd) = run_runtime(
                Path::new(config.runtime_path.to_str().unwrap()),
                node.home.to_str().unwrap(),
                node.port,
                &["--password", &node.password, "--rpc", &node.rpc],
                node.is_runtime_print,
            )?;

            let node_info = NodeInfo {
                process_handle: runtime_process,
                master_fd,
                port: node.port,
                home: node.home,
            };

            if master_node_port.is_none() {
                master_node_port = Some(node_info.port.clone());
            }
            nodes.borrow_mut().push(node_info);
        }

        // Cleanup, boot check, test loading, and running
        for node in nodes.borrow_mut().iter_mut() {
            node.port = wait_until_booted(node.port, 5, 5).await?.unwrap();
        }

        load_tests(&test.test_process_paths, master_node_port.unwrap().clone()).await?;
        run_tests(
            &format!("{:?}", test.test_process_paths),
            master_node_port.unwrap(),
        ).await?;
    }

    Ok(())
}
