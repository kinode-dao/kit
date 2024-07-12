use crate::kinode::process::standard::{ProcessId as WitProcessId};
use crate::kinode::process::{package_name}::{Address as WitAddress, Request as TransferRequest, Response as TransferResponse, WorkerRequest, ProgressRequest, InitializeRequest, ChunkRequest};
use kinode_process_lib::{
    await_message, call_init, get_blob, println,
    vfs::{open_dir, open_file, Directory, File, SeekFrom},
    Address, Message, ProcessId, Request, Response,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name_kebab}-{publisher_dotted_kebab}-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

impl From<Address> for WitAddress {
    fn from(address: Address) -> Self {
        WitAddress {
            node: address.node,
            process: address.process.into(),
        }
    }
}

impl From<ProcessId> for WitProcessId {
    fn from(process: ProcessId) -> Self {
        WitProcessId {
            process_name: process.process_name,
            package_name: process.package_name,
            publisher_node: process.publisher_node,
        }
    }
}
impl From<WitAddress> for Address {
    fn from(address: WitAddress) -> Self {
        Address {
            node: address.node,
            process: address.process.into(),
        }
    }
}

impl From<WitProcessId> for ProcessId {
    fn from(process: WitProcessId) -> Self {
        ProcessId {
            process_name: process.process_name,
            package_name: process.package_name,
            publisher_node: process.publisher_node,
        }
    }
}

const CHUNK_SIZE: u64 = 1048576; // 1MB

fn handle_message(
    our: &Address,
    message: &Message,
    file: &mut Option<File>,
    files_dir: &Directory,
    size: &mut Option<u64>,
) -> anyhow::Result<bool> {
    if !message.is_request() {
        return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
    }

    match message.body().try_into()? {
        WorkerRequest::Initialize(InitializeRequest {
            name,
            target_worker,
        }) => {
            // initialize command from main process,
            // sets up worker, matches on if it's a sender or receiver.
            // target_worker = None, we are receiver, else sender.

            // open/create empty file in both cases.
            let mut active_file =
                open_file(&format!("{}/{}", files_dir.path, &name), true, None)?;

            match target_worker {
                Some(target_worker) => {
                    let target_worker: Address = target_worker.into();
                    // we have a target, chunk the data, and send it.
                    let size = active_file.metadata()?.len;
                    let num_chunks = (size as f64 / CHUNK_SIZE as f64).ceil() as u64;

                    // give the receiving worker a size request so it can track it's progress!
                    Request::new()
                        .body(WorkerRequest::Size(size))
                        .target(target_worker.clone())
                        .send()?;

                    active_file.seek(SeekFrom::Start(0))?;

                    for i in 0..num_chunks {
                        let offset = i * CHUNK_SIZE;
                        let length = CHUNK_SIZE.min(size - offset);

                        let mut buffer = vec![0; length as usize];
                        active_file.read_at(&mut buffer)?;

                        Request::new()
                            .body(WorkerRequest::Chunk(ChunkRequest {
                                name: name.clone(),
                                offset,
                                length,
                            }))
                            .target(target_worker.clone())
                            .blob_bytes(buffer)
                            .send()?;
                    }
                    return Ok(true);
                }
                None => {
                    // waiting for response, store created empty file.
                    *file = Some(active_file);
                    Response::new().body(TransferResponse::Started).send()?;
                }
            }
        }
        // someone sending a chunk to us!
        WorkerRequest::Chunk(ChunkRequest {
            name,
            offset,
            length,
        }) => {
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
                    process: "{package_name}:{package_name}:{publisher}".parse()?,
                };

                Request::new()
                    .body(TransferRequest::Progress(ProgressRequest {
                        name,
                        progress,
                    }))
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
        match await_message() {
            Err(send_error) => println!("worker: got SendError: {send_error}"),
            Ok(ref message) => match handle_message(
                &our,
                message,
                &mut file,
                &files_dir,
                &mut size,
            ) {
                Ok(exit) => {
                    if exit {
                        println!("worker: done: exiting, took {:?}", start.elapsed());
                        break;
                    }
                }
                Err(e) => println!("worker: got error while handling message: {e:?}"),
            }
        }
    }
}
