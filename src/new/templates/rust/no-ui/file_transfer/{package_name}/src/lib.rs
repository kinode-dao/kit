use crate::kinode::process::standard::{ProcessId as WitProcessId};
use crate::kinode::process::{package_name}::{start_download, Address as WitAddress, Request as TransferRequest, Response as TransferResponse, DownloadRequest, ProgressRequest, FileInfo};
use kinode_process_lib::{
    await_message, call_init, println,
    vfs::{create_drive, metadata, open_dir, Directory, FileType},
    Address, Message, ProcessId, Response,
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
        TransferRequest::Download(DownloadRequest { ref name, ref target, is_requestor }) => {
            match start_download(
                &our.clone().into(),
                &message.source().clone().into(),
                name,
                target,
                is_requestor,
            ) {
                Ok(_) => {}
                Err(e) => return Err(anyhow::anyhow!("{e}")),
            }
        }
        TransferRequest::Progress(ProgressRequest { name, progress }) => {
            println!("{} progress: {}%", name, progress);
            Response::new()
                .body(TransferResponse::Progress(Ok(())))
                .send()?;
        }
    }

    Ok(())
}

fn handle_transfer_response(message: &Message) -> anyhow::Result<()> {
    match message.body().try_into()? {
        TransferResponse::ListFiles(ref files) => {
            println!(
                "{}",
                files.iter().
                    fold(format!("{} available files:\nFile\t\tSize (bytes)\n", message.source()), |mut msg, file| {
                        msg.push_str(&format!(
                            "{}\t\t{}", file.name.split('/').last().unwrap(),
                            file.size,
                        ));
                        msg
                    })
            );
        }
        TransferResponse::Download(result) | TransferResponse::Progress(result) => {
            if result.is_err() {
                return Err(anyhow::anyhow!("{}", result.unwrap_err()));
            }
        }
    }

    Ok(())
}

fn handle_message(
    our: &Address,
    message: &Message,
    files_dir: &Directory,
) -> anyhow::Result<()> {
    if message.is_request() {
        handle_transfer_request(our, message, files_dir)
    } else {
        handle_transfer_response(message)
    }
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
