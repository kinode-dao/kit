use std::os::fd::AsRawFd;

use fs_err as fs;
use tokio::io::AsyncBufReadExt;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info, instrument};

use crate::run_tests::types::{BroadcastRecvBool, BroadcastSendBool, NodeCleanupInfo, NodeCleanupInfos, NodeHandles, RecvBool, SendBool};

fn remove_repeated_newlines(input: &str) -> String {
    let re = regex::Regex::new(r"\n\n+").unwrap();
    re.replace_all(input, "\n").into_owned()
}

/// Send SIGINT to the process
#[instrument(level = "trace", skip_all)]
pub fn clean_process_by_pid(process_id: i32) {
    let pid = nix::unistd::Pid::from_raw(process_id);
    match nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)) {
        Ok(nix::sys::wait::WaitStatus::StillAlive) |
        Ok(nix::sys::wait::WaitStatus::Stopped(_, _)) |
        Ok(nix::sys::wait::WaitStatus::Continued(_)) => {
            if let Err(e) = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT) {
                error!("failed to send SIGINT to process: {:?}", e);
            }
        }
        _ => {}
    }
}

/// trigger cleanup if receive signal to kill process
#[instrument(level = "trace", skip_all)]
pub async fn cleanup_on_signal(
    send_to_cleanup: SendBool,
    mut recv_kill_in_cos: BroadcastRecvBool,
) {
    let mut sigalrm = signal(SignalKind::alarm())
        .expect("kit run-tests: failed to set up SIGALRM handler");
    let mut sighup = signal(SignalKind::hangup())
        .expect("kit run-tests: failed to set up SIGHUP handler");
    let mut sigint = signal(SignalKind::interrupt())
        .expect("kit run-tests: failed to set up SIGINT handler");
    let mut sigpipe = signal(SignalKind::pipe())
        .expect("kit run-tests: failed to set up SIGPIPE handler");
    let mut sigquit = signal(SignalKind::quit())
        .expect("kit run-tests: failed to set up SIGQUIT handler");
    let mut sigterm = signal(SignalKind::terminate())
        .expect("kit run-tests: failed to set up SIGTERM handler");
    let mut sigusr1 = signal(SignalKind::user_defined1())
        .expect("kit run-tests: failed to set up SIGUSR1 handler");
    let mut sigusr2 = signal(SignalKind::user_defined2())
        .expect("kit run-tests: failed to set up SIGUSR2 handler");

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

#[instrument(level = "trace", skip_all)]
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
            Some(nh_vec.into_iter().rev())
        },
    };

    for NodeCleanupInfo {
        master_fd,
        process_id,
        home,
        anvil_process,
    } in node_cleanup_infos.iter_mut().rev() {
        // Send Ctrl-C to the process
        info!("Cleaning up {:?}...\r", home);

        if detached {
            // 231222 Note: I (hf) tried to use the `else` method for
            //  both detached and non-detached processes and found it
            //  did not work properly for detached processes; specifically
            //  for `run-tests` that exited early by, e.g., a user input
            //  Ctrl+C.
            if let Err(e) = nix::unistd::write(master_fd.as_raw_fd(), b"\x03") {
                error!("failed to send SIGINT to node: {:?}", e);
            }
        } else {
            clean_process_by_pid(*process_id);
        }

        if let Some(anvil) = anvil_process {
            info!("Cleaning up anvil fakechain...\r");
            clean_process_by_pid(*anvil);
            info!("Cleaned up anvil fakechain.");
        }

        if let Some(ref mut nh) = node_handles {
            if let Some(mut node_handle) = nh.next() {
                node_handle.wait().await.ok();
            }
        }

        if remove_node_files && home.exists() {
            for dir in &["kernel", "kv", "sqlite", "vfs"] {
                let dir = home.join(dir);
                if dir.exists() {
                    fs::remove_dir_all(&dir).unwrap();
                }
            }
        }
        info!("Done cleaning up {:?}.\r", home);
    }
    let _ = send_to_kill.send(should_print_std.is_some_and(|b| b));
}

#[instrument(level = "trace", skip_all)]
pub async fn drain_print_runtime(
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    mut recv_kill: BroadcastRecvBool,
) {
    let mut stdout_reader = tokio::io::BufReader::new(stdout).lines();
    let mut stderr_reader = tokio::io::BufReader::new(stderr).lines();
    let mut stdout_buffer = String::new();
    let mut stderr_buffer = String::new();

    loop {
        tokio::select! {
            Ok(Some(line)) = stdout_reader.next_line() => {
                stdout_buffer.push_str(&line);
                stdout_buffer.push('\n');
            }
            Ok(Some(line)) = stderr_reader.next_line() => {
                stderr_buffer.push_str(&line);
                stderr_buffer.push('\n');
            }
            Ok(should_print_std) = recv_kill.recv() => {
                if should_print_std {
                    let stdout = remove_repeated_newlines(&stdout_buffer);
                    let stderr = remove_repeated_newlines(&stderr_buffer);
                    println!("stdout:\n{}\nstderr:\n{}", stdout, stderr);
                }
                return;
            }
        }
    }
}
