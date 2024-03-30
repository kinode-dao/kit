use kinode::process::standard::get_blob;
use kinode_process_lib::{
    await_message, get_typed_state, http::{
        bind_http_path, bind_ws_path, send_response, send_ws_push, serve_ui, HttpServerRequest,
        StatusCode, WsMessageType,
    }, our_capabilities, print_to_terminal, println, set_state, spawn, vfs::{
        create_drive, create_file, metadata, open_dir, open_file, remove_dir, remove_file, Directory, FileType
    }, Address, LazyLoadBlob, Message, OnExit, ProcessId, Request, Response
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::str::FromStr;

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Serialize, Deserialize, Debug)]
pub struct NodePermission {
    node: String,
    allow: Option<bool>
}

#[derive(Serialize, Deserialize, Debug)]
pub enum KinoRequest {
    ListFiles,
    Download { name: String, target: Address },
    Progress { name: String, progress: u64 },
    Delete { name: String },
    CreateDir { name: String },
    Move { source_path: String, target_path: String },
    ChangePermissions { path: String, perm: Option<NodePermission> },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum KinoResponse {
    ListFiles(Vec<FileInfo>),
    Download { name: String, worker: Address },
    Done,
    Started,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub dir: Option<Vec<FileInfo>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerRequest {
    Initialize {
        name: String,
        target_worker: Option<Address>,
    },
}

fn ls_files(source: &Address, our: &Address, files_dir: &Directory) -> anyhow::Result<Vec<FileInfo>> {
    let entries = files_dir.read()?;
    let files: Vec<FileInfo> = entries
        .iter()
        .filter_map(|file| {
            if source.node != our.node && !node_has_perms_to_path(&source.node, &file.path) {
                return None;
            }
            match file.file_type {
                FileType::File => match metadata(&file.path) {
                    Ok(metadata) => Some(FileInfo {
                        name: file.path.clone(),
                        size: metadata.len,
                        dir: None,
                    }),
                    Err(_) => None,
                },
                FileType::Directory => Some(FileInfo {
                    name: file.path.clone(),
                    size: 0,
                    dir: Some(ls_files(source, our, &open_dir(&file.path, false).unwrap()).unwrap()),
                }),
                _ => None,
            }
        })
        .collect();

    Ok(files)
}

fn node_has_perms_to_path(node: &String, path: &String) -> bool {
    let path = path.split("/files/").last().unwrap_or(path);
    let state = get_typed_state(|bytes| Ok(serde_json::from_slice::<FileTransferState>(&bytes)?)).unwrap_or(empty_state());
    // println!("checking perms for path {} from node {} among {:?}", path, node, state.permissions);
    let permissions = state.permissions.get(path);
    match permissions {
        Some(perms) => {
            if let Some(&is_permitted) = perms.get(node) {
                // println!("{} is permitted to access {}? {}", node, path, is_permitted);
                return is_permitted;
            }
            // If the node is not explicitly mentioned, it's allowed if all other permissions are false (forbiddances only).
            let is_permitted = perms.values().all(|&perm| !perm);
            // println!("{} is? permitted to access {}? {}", node, path, is_permitted);
            is_permitted
        },
        None => true, // If there are no permissions set for this path, it's accessible to all.
    }
}

fn handle_kinofiles_request(
    our: &Address,
    source: &Address,
    body: &Vec<u8>,
    files_dir: &Directory,
    channel_id: &mut u32,
) -> anyhow::Result<()> {
    let Ok(kino_req) = serde_json::from_slice::<KinoRequest>(body) else {
        // println!("{package_name}: error: failed to parse request: {}", String::from_utf8_lossy(&body));
        return Ok(())
    };

    match kino_req {
        KinoRequest::ListFiles => {
            let files = ls_files(source, our, files_dir)?;

            Response::new()
                .body(serde_json::to_vec(&KinoResponse::ListFiles(files))?)
                .send()?;
        }
        KinoRequest::Download { name, target } => {
            // check if source has permission to see the file. if so, they may also download it
            let files_available_to_node = ls_files(source, our, files_dir)?;
            if !files_available_to_node.iter().any(|file| file.name == name) {
                return Ok(());
            }

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

            match source.node == our.node {
                true => {
                    // we want to download a file
                    let _resp = Request::new()
                        .body(serde_json::to_vec(&WorkerRequest::Initialize {
                            name: name.clone(),
                            target_worker: None,
                        })?)
                        .target(&our_worker_address)
                        .send_and_await_response(5)??;

                    // send our initialized worker address to the other node
                    Request::new()
                        .body(serde_json::to_vec(&KinoRequest::Download {
                            name: name.clone(),
                            target: our_worker_address,
                        })?)
                        .target(&target)
                        .send()?;
                }
                false => {
                    // they want to download a file
                    Request::new()
                        .body(serde_json::to_vec(&WorkerRequest::Initialize {
                            name: name.clone(),
                            target_worker: Some(target),
                        })?)
                        .target(&our_worker_address)
                        .send()?;
                }
            }
        }
        KinoRequest::Progress { name, progress } => {
            // print out in terminal and pipe to UI via websocket
            println!("{package_name}: file: {} progress: {}%", name, progress);
            let ws_blob = LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::json!({
                    "kind": "progress",
                    "data": {
                        "name": name,
                        "progress": progress,
                    }
                })
                .to_string()
                .as_bytes()
                .to_vec(),
            };
            send_ws_push(
                channel_id.clone(),
                WsMessageType::Text,
                ws_blob,
            );
        }
        KinoRequest::Delete { name } => {
            if source.node != our.node {
                return Ok(());
            }
            println!("{package_name}: deleting file: {}", name);
            let meta = metadata(&name)?;
            if meta.file_type == FileType::Directory {
                remove_dir(&name)?;
            } else {
                remove_file(&name)?;
            }
            push_file_update_via_ws(channel_id);
        }
        KinoRequest::CreateDir { name } => {
            if source.node != our.node {
                return Ok(());
            }
            let path = format!("{}/{}", files_dir.path, name);
            println!("{package_name}: creating directory: {}", path);
            open_dir(&path, true)?;
            push_file_update_via_ws(channel_id);
        }
        KinoRequest::Move { source_path, target_path } => {
            if source.node != our.node {
                return Ok(());
            }
            println!("{package_name}: moving file: {} to {}", source_path, target_path);
            let filename = source_path.split("/").last().unwrap_or(&source_path);
            let dest_path = format!("{}/{}", target_path, filename).replace("//", "/");
            if dest_path == source_path {
                return Ok(());
            }
            let file = open_file(&source_path, false)?;
            let dest_file = create_file(&dest_path)?;
            dest_file.write(&file.read()?)?;
            remove_file(&source_path)?;
            push_file_update_via_ws(channel_id);
        }
        KinoRequest::ChangePermissions { path, perm } => {
            if source.node != our.node {
                return Err(anyhow::anyhow!("permit path request from non-local node"));
            }
            println!("{package_name}: changing perms for path: {}", path);
            let mut state = get_typed_state(|bytes| Ok(serde_json::from_slice::<FileTransferState>(&bytes)?))
                .unwrap_or(empty_state());
            match perm {
                None => {
                    // println!("{package_name}: removing all perms for file");
                    state.permissions.remove(&path);
                },
                Some(NodePermission { node, allow }) => {
                    let path_perms = state.permissions
                        .entry(path.clone())
                        .or_insert_with(|| HashMap::new());
                    if let Some(new_perm) = allow {
                        // println!("{package_name}: adding perms for node: {} to path: {} with perm: {}", node, path, new_perm);
                        path_perms.insert(node.clone(), new_perm);
                    } else {
                        // println!("{package_name}: removing perms for node: {} from path: {}", node, path);
                        path_perms.remove(&node);
                    }
                },
            }
            // println!("{package_name}: new perms: {:?}", state);
            set_state(&serde_json::to_vec(&state)?);
            push_state_via_ws(channel_id);
        }
    }

    Ok(())
}

fn handle_http_request(
    our: &Address,
    source: &Address,
    body: &Vec<u8>,
    files_dir: &Directory,
    our_channel_id: &mut u32,
) -> anyhow::Result<()> {
    let http_request = serde_json::from_slice::<HttpServerRequest>(body)?;

    match http_request {
        HttpServerRequest::Http(request) => {
            match request.method()?.as_str() {
                "GET" => {
                    // /?node=akira.os
                    if let Some(remote_node) = request.query_params().get("node") {
                        let remote_node = Address {
                            node: remote_node.clone(),
                            process: our.process.clone(),
                        };

                        let state = get_typed_state(|bytes| Ok(serde_json::from_slice::<FileTransferState>(&bytes)?))
                            .unwrap_or(empty_state());

                        if !state.known_nodes.contains(&remote_node.node) {
                            let mut state = state;
                            state.known_nodes.push(remote_node.node.clone());
                            set_state(&serde_json::to_vec(&state)?);
                        }

                        let resp = Request::new()
                            .body(serde_json::to_vec(&KinoRequest::ListFiles)?)
                            .target(&remote_node)
                            .send_and_await_response(5)??;

                        handle_kinofiles_response(source, &resp.body().to_vec(), true)?;
                    }

                    let files = ls_files(source, our, files_dir)?;
                    let mut headers = HashMap::new();
                    headers.insert("Content-Type".to_string(), "application/json".to_string());

                    let body = serde_json::to_vec(&KinoResponse::ListFiles(files))?;

                    send_response(StatusCode::OK, Some(headers), body);
                }
                "POST" => {
                    if source.node != our.node {
                        return Ok(());
                    }
                    let headers = request.headers();
                    let content_type = headers
                        .get("Content-Type")
                        .ok_or_else(|| anyhow::anyhow!("upload, Content-Type header not found"))?
                        .to_str()
                        .map_err(|_| anyhow::anyhow!("failed to convert Content-Type to string"))?;

                    let body = get_blob()
                        .ok_or_else(|| anyhow::anyhow!("failed to get blob"))?
                        .bytes;

                    let boundary_parts: Vec<&str> = content_type.split("boundary=").collect();
                    let boundary = match boundary_parts.get(1) {
                        Some(boundary) => boundary,
                        None => {
                            return Err(anyhow::anyhow!(
                                "upload fail, no boundary found in POST content type"
                            ));
                        }
                    };

                    let data = Cursor::new(body.clone());

                    let mut multipart = multipart::server::Multipart::with_body(data, *boundary);
                    while let Some(mut field) = multipart.read_entry()? {
                        if let Some(filename) = field.headers.filename.clone() {
                            let mut buffer = Vec::new();
                            field.data.read_to_end(&mut buffer)?;
                            println!("{package_name}: uploaded file {} with size {}", filename, buffer.len());
                            let file_path = format!("{}/{}", files_dir.path, filename);
                            let file = create_file(&file_path)?;
                            file.write(&buffer)?;

                            let ws_blob = LazyLoadBlob {
                                mime: Some("application/json".to_string()),
                                bytes: serde_json::json!({
                                    "kind": "uploaded",
                                    "data": {
                                        "name": filename,
                                        "size": buffer.len(),
                                    }
                                })
                                .to_string()
                                .as_bytes()
                                .to_vec(),
                            };

                            send_ws_push(
                                our_channel_id.clone(),
                                WsMessageType::Text,
                                ws_blob,
                            );
                        }
                    }

                    let mut headers = HashMap::new();
                    headers.insert("Content-Type".to_string(), "application/json".to_string());
                    send_response(StatusCode::OK, Some(headers), vec![]);
                }
                _ => {}
            }
        }
        HttpServerRequest::WebSocketClose(_) => {}
        HttpServerRequest::WebSocketOpen { channel_id, .. } => {
            *our_channel_id = channel_id;

            push_state_via_ws(our_channel_id);
        }
        HttpServerRequest::WebSocketPush { message_type, .. } => {
            if message_type != WsMessageType::Binary {
                return Ok(());
            }
            let Some(blob) = get_blob() else {
                return Ok(());
            };
            handle_kinofiles_request(our, source, &blob.bytes, files_dir, our_channel_id)?
        }
    }
    Ok(())
}

fn push_state_via_ws(channel_id: &mut u32) {
    send_ws_push(
        channel_id.clone(),
        WsMessageType::Text,
        LazyLoadBlob {
            mime: Some("application/json".to_string()),
            bytes: serde_json::json!({
                "kind": "state",
                "data": get_typed_state(|bytes| Ok(serde_json::from_slice::<FileTransferState>(&bytes)?))
                    .unwrap_or(empty_state())
            })
            .to_string()
            .as_bytes()
            .to_vec()
        }
    )
}

fn push_file_update_via_ws(channel_id: &mut u32) {
    send_ws_push(
        channel_id.clone(),
        WsMessageType::Text,
        LazyLoadBlob {
            mime: Some("application/json".to_string()),
            bytes: serde_json::json!({
                "kind": "file_update",
                "data": ""
            })
            .to_string()
            .as_bytes()
            .to_vec()
        }
    )
}

fn push_error_via_ws(channel_id: &mut u32, error: String) {
    send_ws_push(
        channel_id.clone(),
        WsMessageType::Text,
        LazyLoadBlob {
            mime: Some("application/json".to_string()),
            bytes: serde_json::json!({
                "kind": "error",
                "data": error
            })
            .to_string()
            .as_bytes()
            .to_vec()
        }
    )
}

fn handle_kinofiles_response(source: &Address, body: &Vec<u8>, is_http: bool) -> anyhow::Result<()> {
    let Ok(kino_res) = serde_json::from_slice::<KinoResponse>(body) else {
        // println!("{package_name}: error: failed to parse response: {}", String::from_utf8_lossy(&body));
        return Ok(());
    };

    match kino_res {
        KinoResponse::ListFiles(files) => {
            println!("{package_name}: got files from node: {:?} ,files: {:?}", source, files);

            if is_http {
                let mut headers = HashMap::new();
                headers.insert("Content-Type".to_string(), "application/json".to_string());

                let body = serde_json::to_vec(&KinoResponse::ListFiles(files))?;

                send_response(StatusCode::OK, Some(headers), body)
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_message(
    our: &Address,
    files_dir: &Directory,
    channel_id: &mut u32,
) -> anyhow::Result<()> {
    let message = await_message()?;

    let http_server_address = ProcessId::from_str("http_server:distro:sys").unwrap();

    match message {
        Message::Response {
            ref source,
            ref body,
            ..
        } => handle_kinofiles_response(source, body, false),
        Message::Request {
            ref source,
            ref body,
            ..
        } => {
            if source.process == http_server_address {
                handle_http_request(&our, source, body, files_dir, channel_id)?
            }
            handle_kinofiles_request(&our, source, body, files_dir, channel_id)
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct FileTransferState {
    pub known_nodes: Vec<String>,
    pub permissions: HashMap<String, HashMap<String, bool>>,
}

fn empty_state() -> FileTransferState {
    FileTransferState {
        known_nodes: vec![],
        permissions: HashMap::new(),
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        println!("{package_name}: begin");

        let our = Address::from_str(&our).unwrap();
        let drive_path = create_drive(our.package_id(), "files").unwrap();
        let state = get_typed_state(|bytes| Ok(serde_json::from_slice::<FileTransferState>(&bytes)?))
            .unwrap_or(empty_state());
        set_state(&serde_json::to_vec(&state).unwrap_or(vec![]));
        let files_dir = open_dir(&drive_path, false).unwrap();

        serve_ui(&our, &"ui", true, false, vec!["/"]).unwrap();
        bind_http_path("/files", false, false).unwrap();
        bind_ws_path("/", false, false).unwrap();

        let mut channel_id: u32 = 1854;

        loop {
            match handle_message(&our, &files_dir, &mut channel_id) {
                Ok(()) => {}
                Err(e) => {
                    print_to_terminal(2, format!("{package_name}: error: {:?}", e).as_str());
                    push_error_via_ws(&mut channel_id, e.to_string());
                }
            };
        }
    }
}
