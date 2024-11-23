use std::os::unix::io::OwnedFd;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub runtime: Runtime,
    pub runtime_build_release: bool,
    pub persist_home: bool,
    pub always_print_node_output: bool,
    pub tests: Vec<Test>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Runtime {
    FetchVersion(String),
    RepoPath(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Test {
    pub dependency_package_paths: Vec<PathBuf>,
    pub setup_packages: Vec<SetupPackage>,
    pub setup_scripts: Vec<String>,
    pub test_package_paths: Vec<PathBuf>,
    pub test_scripts: Vec<String>,
    pub timeout_secs: u64,
    pub fakechain_router: u16,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupPackage {
    pub path: PathBuf,
    pub run: bool,
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

pub struct SetupCleanupReturn {
    pub send_to_cleanup: tokio::sync::mpsc::UnboundedSender<bool>,
    pub send_to_kill: BroadcastSendBool,
    pub task_handles: Vec<tokio::task::JoinHandle<()>>,
    pub cleanup_context: CleanupContext,
    pub master_node_port: Option<u16>,
    pub node_cleanup_infos: NodeCleanupInfos,
    pub node_handles: NodeHandles,
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
