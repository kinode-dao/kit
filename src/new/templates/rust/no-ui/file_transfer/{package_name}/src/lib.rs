use crate::kinode::process::{package_name}_{publisher_dotted_snake}_api_v0::{Address as WitAddress, TransferRequest, TransferResponse, WorkerRequest, DownloadRequest, ProgressRequest, FileInfo, InitializeRequest, ChunkRequest};
use kinode_process_lib::{
    await_message, call_init, our_capabilities, println, spawn,
    vfs::{create_drive, metadata, open_dir, Directory, FileType},
    Address, OnExit, Request, Response,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name}",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

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
    source: &Address,
    body: &[u8],
    files_dir: &Directory,
) -> anyhow::Result<()> {
    match serde_json::from_slice(body)? {
        TransferRequest::ListFiles => {
            let files = ls_files(files_dir)?;

            Response::new()
                .body(serde_json::to_vec(&TransferResponse::ListFiles(files))?)
                .send()?;
        }
        TransferRequest::Download(DownloadRequest { name, ref target }) => {
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
            let our_worker_wit_address = serde_json::from_str(&serde_json::to_string(&our_worker_address)?)?;

            if source.node == our.node {
                // we want to download a file
                let _resp = Request::new()
                    .body(serde_json::to_vec(&WorkerRequest::Initialize(InitializeRequest {
                        name: name.clone(),
                        target_worker: None,
                    }))?)
                    .target(&our_worker_address)
                    .send_and_await_response(5)??;

                // send our initialized worker address to the other node
                let target = serde_json::from_str(&serde_json::to_string(target)?)?;
                Request::new()
                    .body(serde_json::to_vec(&TransferRequest::Download(DownloadRequest {
                        name: name.clone(),
                        target: our_worker_wit_address,
                    }))?)
                    .target(&target)
                    //.target(&target.into())
                    .send()?;
            } else {
                // they want to download a file
                Request::new()
                    .body(serde_json::to_vec(&WorkerRequest::Initialize(InitializeRequest {
                        name: name.clone(),
                        target_worker: Some(target.clone()),
                    }))?)
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

fn handle_message(our: &Address, files_dir: &Directory) -> anyhow::Result<()> {
    let message = await_message()?;
    handle_transfer_request(our, message.source(), message.body(), files_dir)
}

call_init!(init);
fn init(our: Address) {
    println!("begin");

    let drive_path = create_drive(our.package_id(), "files", None).unwrap();
    let files_dir = open_dir(&drive_path, false, None).unwrap();

    loop {
        match handle_message(&our, &files_dir) {
            Ok(()) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        };
    }
}
