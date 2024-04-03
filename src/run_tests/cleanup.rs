use std::os::fd::AsRawFd;

use fs_err as fs;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info};

use crate::run_tests::types::{BroadcastRecvBool, BroadcastSendBool, NodeCleanupInfo, NodeCleanupInfos, NodeHandles, RecvBool, SendBool};

fn remove_repeated_newlines(input: &str) -> String {
    let re = regex::Regex::new(r"\n\n+").unwrap();
    re.replace_all(input, "\n").into_owned()
}

/// trigger cleanup if receive signal to kill process
pub async fn cleanup_on_signal(
    send_to_cleanup: SendBool,
    mut recv_kill_in_cos: BroadcastRecvBool,
) {
    let mut sigalrm = signal(SignalKind::alarm()).expect("kit run-tests: failed to set up SIGALRM handler");
    let mut sighup = signal(SignalKind::hangup()).expect("kit run-tests: failed to set up SIGHUP handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("kit run-tests: failed to set up SIGINT handler");
    let mut sigpipe = signal(SignalKind::pipe()).expect("kit run-tests: failed to set up SIGPIPE handler");
    let mut sigquit = signal(SignalKind::quit()).expect("kit run-tests: failed to set up SIGQUIT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("kit run-tests: failed to set up SIGTERM handler");
    let mut sigusr1 = signal(SignalKind::user_defined1()).expect("kit run-tests: failed to set up SIGUSR1 handler");
    let mut sigusr2 = signal(SignalKind::user_defined2()).expect("kit run-tests: failed to set up SIGUSR2 handler");

    tokio::select! {
        _ = sigalrm.recv() => error!("kit cleanup got SIGALRM\r"),
        _ = sighup.recv() =>  error!("kit cleanup got SIGHUP\r"),
        _ = sigint.recv() =>  error!("kit cleanup got SIGINT\r"),
        _ = sigpipe.recv() => error!("kit cleanup got SIGPIPE\r"),
        _ = sigquit.recv() => error!("kit cleanup got SIGQUIT\r"),
        _ = sigterm.recv() => error!("kit cleanup got SIGTERM\r"),
        _ = sigusr1.recv() => error!("kit cleanup got SIGUSR1\r"),
        _ = sigusr2.recv() => error!("kit cleanup got SIGUSR2\r"),
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
    remove_node_files: bool,
) {
    // Block until get cleanup request.
    let should_print_std = recv_in_cleanup.recv().await;

    let mut node_cleanup_infos = node_cleanup_infos.lock().await;
    let mut node_handles = match node_handles {
        None => None,
        Some(nh) => {
            let mut nh = nh.lock().await;
            let nh_vec = std::mem::replace(&mut *nh, Vec::new());
            Some(nh_vec.into_iter())
        },
    };

    for NodeCleanupInfo { master_fd, process_id, home } in node_cleanup_infos.iter_mut() {
        // Send Ctrl-C to the process
        info!("Cleaning up {:?}...\r", home);
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

        if let Some(ref mut nh) = node_handles {
            if should_print_std.is_some_and(|b| b) {
                let output = nh.next().and_then(|n| n.wait_with_output().ok()).unwrap();
                let stdout = remove_repeated_newlines(&String::from_utf8_lossy(&output.stdout));
                let stderr = remove_repeated_newlines(&String::from_utf8_lossy(&output.stderr));
                println!("stdout:\n{}\nstderr:\n{}", stdout, stderr);
            } else {
                nh.next().and_then(|mut n| n.wait().ok()).unwrap();
            }
        }

        if remove_node_files && home.exists() {
            for dir in &["kernel", "kv", "sqlite", "vfs"] {
                let dir = home.join(dir);
                if dir.exists() {
                    fs::remove_dir_all(&home.join(dir)).unwrap();
                }
            }
        }
        info!("Done cleaning up {:?}.\r", home);
    }
    let _ = send_to_kill.send(true);
}
