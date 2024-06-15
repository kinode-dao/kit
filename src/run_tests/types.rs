use std::os::unix::io::OwnedFd;
use std::path::PathBuf;
//use std::process::Child;
use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub runtime: Runtime,
    pub runtime_build_release: bool,
    pub tests: Vec<Test>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Runtime {
    FetchVersion(String),
    RepoPath(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Test {
    pub setup_package_paths: Vec<PathBuf>,
    pub test_packages: Vec<TestPackage>,
    pub setup_scripts: Vec<Script>,
    pub test_scripts: Vec<Script>,
    pub timeout_secs: u64,
    pub fakechain_router: u16,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPackage {
    pub path: PathBuf,
    pub grant_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    pub path: PathBuf,
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub port: u16,
    pub home: PathBuf,
    pub fake_node_name: String,
    pub password: Option<String>,
    pub rpc: Option<String>,
    pub runtime_verbosity: Option<u8>,
}

pub type NodeHandles = Arc<Mutex<Vec<Child>>>;
pub type NodeCleanupInfos = Arc<Mutex<Vec<NodeCleanupInfo>>>;

pub type RecvBool = tokio::sync::mpsc::UnboundedReceiver<bool>;
pub type SendBool = tokio::sync::mpsc::UnboundedSender<bool>;
pub type BroadcastRecvBool = tokio::sync::broadcast::Receiver<bool>;
pub type BroadcastSendBool = tokio::sync::broadcast::Sender<bool>;

#[derive(Debug)]
pub struct NodeCleanupInfo {
    pub master_fd: OwnedFd,
    pub process_id: i32,
    pub home: PathBuf,
    pub anvil_process: Option<i32>,
    pub other_processes: Vec<i32>,
}

pub struct CleanupContext {
    pub send_to_cleanup: SendBool,
}

impl CleanupContext {
    pub fn new(send_to_cleanup: SendBool) -> Self {
        CleanupContext { send_to_cleanup }
    }
}

impl Drop for CleanupContext {
    fn drop(&mut self) {
        let _ = self.send_to_cleanup.send(true);
    }
}
