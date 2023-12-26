use std::fs;
use std::os::fd::AsRawFd;

use tokio::signal::unix::{signal, SignalKind};

use crate::run_tests::types::{BroadcastRecvBool, BroadcastSendBool, NodeCleanupInfo, NodeCleanupInfos, NodeHandles, RecvBool, SendBool};

/// trigger cleanup if receive signal to kill process
pub async fn cleanup_on_signal(
    send_to_cleanup: SendBool,
    mut recv_kill_in_cos: BroadcastRecvBool,
) {
    let mut sigalrm = signal(SignalKind::alarm()).expect("uqdev run-tests: failed to set up SIGALRM handler");
    let mut sighup = signal(SignalKind::hangup()).expect("uqdev run-tests: failed to set up SIGHUP handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("uqdev run-tests: failed to set up SIGINT handler");
    let mut sigpipe = signal(SignalKind::pipe()).expect("uqdev run-tests: failed to set up SIGPIPE handler");
    let mut sigquit = signal(SignalKind::quit()).expect("uqdev run-tests: failed to set up SIGQUIT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("uqdev run-tests: failed to set up SIGTERM handler");
    let mut sigusr1 = signal(SignalKind::user_defined1()).expect("uqdev run-tests: failed to set up SIGUSR1 handler");
    let mut sigusr2 = signal(SignalKind::user_defined2()).expect("uqdev run-tests: failed to set up SIGUSR2 handler");

    tokio::select! {
        _ = sigalrm.recv() => println!("uqdev cleanup got SIGALRM\r"),
        _ = sighup.recv() =>  println!("uqdev cleanup got SIGHUP\r"),
        _ = sigint.recv() =>  println!("uqdev cleanup got SIGINT\r"),
        _ = sigpipe.recv() => println!("uqdev cleanup got SIGPIPE\r"),
        _ = sigquit.recv() => println!("uqdev cleanup got SIGQUIT\r"),
        _ = sigterm.recv() => println!("uqdev cleanup got SIGTERM\r"),
        _ = sigusr1.recv() => println!("uqdev cleanup got SIGUSR1\r"),
        _ = sigusr2.recv() => println!("uqdev cleanup got SIGUSR2\r"),
        _ = recv_kill_in_cos.recv() => {},
    }

    let _ = send_to_cleanup.send(true);
}

pub async fn cleanup(
    mut recv_in_cleanup: RecvBool,
    send_to_kill: BroadcastSendBool,
    node_cleanup_infos: NodeCleanupInfos,
    node_handles: Option<NodeHandles>,
    detached: bool,
) {
    // Block until get cleanup request.
    recv_in_cleanup.recv().await;

    let mut node_cleanup_infos = node_cleanup_infos.lock().await;

    for (i, NodeCleanupInfo { master_fd, process_id, home }) in node_cleanup_infos.iter_mut().enumerate() {
        // Send Ctrl-C to the process
        println!("Cleaning up {:?}...\r", home);
        if detached {
            // 231222 Note: I (hf) tried to use the `else` method for
            //  both detached and non-detached processes and found it
            //  did not work properly for detached processes; specifically
            //  for `run-tests` that exited early by, e.g., a user input
            //  Ctrl+C.
            nix::unistd::write(master_fd.as_raw_fd(), b"\x03").unwrap();
        } else {
            let pid = nix::unistd::Pid::from_raw(*process_id);
            match nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)) {
                Ok(nix::sys::wait::WaitStatus::StillAlive) |
                Ok(nix::sys::wait::WaitStatus::Stopped(_, _)) |
                Ok(nix::sys::wait::WaitStatus::Continued(_)) => {
                    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT)
                        .expect("SIGINT failed");
                }
                _ => {}
            }
        }

        if let Some(ref node_handles) = node_handles {
            let mut nh = node_handles.lock().await;
            nh[i].wait().unwrap();
        }

        if home.exists() {
            for dir in &["kernel", "kv", "sqlite", "vfs"] {
                let dir = home.join(dir);
                if dir.exists() {
                    fs::remove_dir_all(&home.join(dir)).unwrap();
                }
            }
        }
        println!("Done cleaning up {:?}.\r", home);
    }
    let _ = send_to_kill.send(true);
}
