use std::os::unix::io::OwnedFd;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;

use tokio::sync::Mutex;

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub runtime: Runtime,
    pub runtime_build_verbose: bool,
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
    pub test_package_paths: Vec<PathBuf>,
    pub package_build_verbose: bool,
    pub timeout_secs: u64,
    pub network_router: NetworkRouter,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRouter {
    pub port: u16,
    pub defects: NetworkRouterDefects,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkRouterDefects {
    None,
    // TODO: add Latency, Dropping, ..., All
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub port: u16,
    pub home: PathBuf,
    pub fake_node_name: Option<String>,
    pub password: Option<String>,
    pub rpc: Option<String>,
    pub runtime_verbose: bool,
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
}

pub struct CleanupContext {
    pub send_to_cleanup: SendBool,
}

impl CleanupContext {
    pub fn new(
        send_to_cleanup: SendBool,
    ) -> Self {
        CleanupContext { send_to_cleanup }
    }
}

impl Drop for CleanupContext {
    fn drop(&mut self) {
        let _ = self.send_to_cleanup.send(true);
    }
}
