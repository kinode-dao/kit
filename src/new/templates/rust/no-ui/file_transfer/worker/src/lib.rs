use serde::{Deserialize, Serialize};
use std::str::FromStr;

use kinode_process_lib::{
    await_message, call_init, get_blob, println,
    vfs::{open_dir, open_file, Directory, File, SeekFrom},
    Address, Message, ProcessId, Request, Response,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
});

const CHUNK_SIZE: u64 = 1048576; // 1MB

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerRequest {
    Initialize {
        name: String,
        target_worker: Option<Address>,
    },
    Chunk {
        name: String,
        offset: u64,
        length: u64,
    },
    Size(u64),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TransferRequest {
    ListFiles,
    Download { name: String, target: Address },
    Progress { name: String, progress: u64 },
}

fn handle_message(
    our: &Address,
    file: &mut Option<File>,
    files_dir: &Directory,
    size: &mut Option<u64>,
) -> anyhow::Result<bool> {
    let message = await_message()?;

    match message {
        Message::Request {
            ref body,
            ..
        } => {
            let request = serde_json::from_slice::<WorkerRequest>(body)?;

            match request {
                WorkerRequest::Initialize {
                    name,
                    target_worker,
                } => {
                    // initialize command from main process,
                    // sets up worker, matches on if it's a sender or receiver.
                    // target_worker = None, we are receiver, else sender.

                    // open/create empty file in both cases.
                    let mut active_file =
                        open_file(&format!("{}/{}", files_dir.path, &name), true, None)?;

                    match target_worker {
                        Some(target_worker) => {
                            // we have a target, chunk the data, and send it.
                            let size = active_file.metadata()?.len;
                            let num_chunks = (size as f64 / CHUNK_SIZE as f64).ceil() as u64;

                            // give the receiving worker a size request so it can track it's progress!
                            Request::new()
                                .body(serde_json::to_vec(&WorkerRequest::Size(size))?)
                                .target(target_worker.clone())
                                .send()?;

                            active_file.seek(SeekFrom::Start(0))?;

                            for i in 0..num_chunks {
                                let offset = i * CHUNK_SIZE;
                                let length = CHUNK_SIZE.min(size - offset);

                                let mut buffer = vec![0; length as usize];
                                active_file.read_at(&mut buffer)?;

                                Request::new()
                                    .body(serde_json::to_vec(&WorkerRequest::Chunk {
                                        name: name.clone(),
                                        offset,
                                        length,
                                    })?)
                                    .target(target_worker.clone())
                                    .blob_bytes(buffer)
                                    .send()?;
                            }
                            Response::new().body(serde_json::to_vec(&"Done")?).send()?;
                            return Ok(true);
                        }
                        None => {
                            // waiting for response, store created empty file.
                            *file = Some(active_file);
                            Response::new()
                                .body(serde_json::to_vec(&"Started")?)
                                .send()?;
                        }
                    }
                }
                // someone sending a chunk to us!
                WorkerRequest::Chunk {
                    name,
                    offset,
                    length,
                } => {
                    let file = match file {
                        Some(file) => file,
                        None => {
                            return Err(anyhow::anyhow!(
                                "{package_name} worker: receive error: no file initialized"
                            ));
                        }
                    };

                    let bytes = match get_blob() {
                        Some(blob) => blob.bytes,
                        None => {
                            return Err(anyhow::anyhow!("{package_name} worker: receive error: no blob"));
                        }
                    };

                    // file.seek(SeekFrom::Start(offset))?; seek not necessary if the sends come in order.
                    file.write_all(&bytes)?;
                    // if sender has sent us a size, give a progress update to main transfer!
                    if let Some(size) = size {
                        let progress = ((offset + length) as f64 / *size as f64 * 100.0) as u64;

                        // send update to main process
                        let main_app = Address {
                            node: our.node.clone(),
                            process: ProcessId::from_str(
                                "{package_name}:{package_name}:{publisher}",
                            )?,
                        };

                        Request::new()
                            .body(serde_json::to_vec(&TransferRequest::Progress {
                                name,
                                progress,
                            })?)
                            .target(&main_app)
                            .send()?;

                        if progress >= 100 {
                            return Ok(true);
                        }
                    }
                }
                WorkerRequest::Size(incoming_size) => {
                    *size = Some(incoming_size);
                }
            }
        }
        _ => {
            println!("worker: got something else than request...");
        }
    }
    Ok(false)
}

call_init!(init);
fn init(our: Address) {
    println!("worker: begin");
    let start = std::time::Instant::now();

    let drive_path = format!("{}/files", our.package_id());
    let files_dir = open_dir(&drive_path, false, None).unwrap();

    let mut file: Option<File> = None;
    let mut size: Option<u64> = None;

    loop {
        match handle_message(&our, &mut file, &files_dir, &mut size) {
            Ok(exit) => {
                if exit {
                    println!(
                        "worker: done: exiting, took {:?}",
                        start.elapsed()
                    );
                    break;
                }
            }
            Err(e) => {
                println!("worker: error: {:?}", e);
            }
        };
    }
}
