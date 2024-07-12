use crate::kinode::process::standard::{ProcessId as WitProcessId};
use crate::kinode::process::{package_name}::{Address as WitAddress, Request as TransferRequest, Response as TransferResponse, WorkerRequest, DownloadRequest, ProgressRequest, FileInfo, InitializeRequest};
use kinode_process_lib::{
    await_message, call_init, our_capabilities, println, spawn,
    vfs::{create_drive, metadata, open_dir, Directory, FileType},
    Address, Message, OnExit, ProcessId, Request, Response,
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

fn ls_files(files_dir: &Directory) -> anyhow::Result<Vec<FileInfo>> {
    let entries = files_dir.read()?;
    let files: Vec<FileInfo> = entries
        .iter()
        .filter_map(|file| match file.file_type {
            FileType::File => match metadata(&file.path, None) {
                Ok(metadata) => Some(FileInfo {
                    name: file.path.clone(),
                    size: metadata.len,
                }),
                Err(_) => None,
            },
            _ => None,
        })
        .collect();

    Ok(files)
}

fn handle_transfer_request(
    our: &Address,
    message: &Message,
    files_dir: &Directory,
) -> anyhow::Result<()> {
    match message.body().try_into()? {
        TransferRequest::ListFiles => {
            let files = ls_files(files_dir)?;

            Response::new()
                .body(TransferResponse::ListFiles(files))
                .send()?;
        }
        TransferRequest::Download(DownloadRequest { name, target }) => {
            // spin up a worker, initialize based on whether it's a downloader or a sender.
            let our_worker = spawn(
                None,
                &format!("{}/pkg/worker.wasm", our.package_id()),
                OnExit::None,
                our_capabilities(),
                vec![],
                false,
            )?;

            let our_worker_address = Address {
                node: our.node.clone(),
                process: our_worker,
            };

            if message.source().node == our.node {
                // we want to download a file
                let _resp = Request::new()
                    .body(WorkerRequest::Initialize(InitializeRequest {
                        name: name.clone(),
                        target_worker: None,
                    }))
                    .target(&our_worker_address)
                    .send_and_await_response(5)??;

                // send our initialized worker address to the other node
                Request::new()
                    .body(TransferRequest::Download(DownloadRequest {
                        name: name.clone(),
                        target: our_worker_address.into(),
                    }))
                    .target::<Address>(target.clone().into())
                    .send()?;
            } else {
                // they want to download a file
                Request::new()
                    .body(WorkerRequest::Initialize(InitializeRequest {
                        name: name.clone(),
                        target_worker: Some(target),
                    }))
                    .target(&our_worker_address)
                    .send()?;
            }
        }
        TransferRequest::Progress(ProgressRequest { name, progress }) => {
            println!("{} progress: {}%", name, progress);
        }
    }

    Ok(())
}

fn handle_message(
    our: &Address,
    message: &Message,
    files_dir: &Directory,
) -> anyhow::Result<()> {
    handle_transfer_request(our, message, files_dir)
}

call_init!(init);
fn init(our: Address) {
    println!("begin");

    let drive_path = create_drive(our.package_id(), "files", None).unwrap();
    let files_dir = open_dir(&drive_path, false, None).unwrap();

    loop {
        match await_message() {
            Err(send_error) => println!("got SendError: {send_error}"),
            Ok(ref message) => match handle_message(&our, message, &files_dir) {
                Ok(_) => {}
                Err(e) => println!("got error while handling message: {e:?}"),
            }
        }
    }
}
