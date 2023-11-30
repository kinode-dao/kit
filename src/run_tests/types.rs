use std::cell::RefCell;
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::rc::Rc;

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub runtime_path: PathBuf,
    pub runtime_build_verbose: bool,
    pub tests: Vec<Test>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Test {
    pub setup_package_paths: Vec<PathBuf>,
    pub test_package_paths: Vec<PathBuf>,
    pub package_build_verbose: bool,
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
    pub password: String,
    pub rpc: String,
    pub runtime_verbose: bool,
}

#[derive(Debug)]
pub struct NodeInfo {
    pub process_handle: Child,
    pub master_fd: OwnedFd,
    pub port: u16,
    pub home: PathBuf,
}

pub struct CleanupContext {
    pub nodes: Rc<RefCell<Vec<NodeInfo>>>,
}

impl CleanupContext {
    pub fn new(nodes: Rc<RefCell<Vec<NodeInfo>>>) -> Self {
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
