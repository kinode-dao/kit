use serde::{Serialize, Deserialize};

use uqbar_process_lib::{await_message, println, Address, Message, ProcessId, Request, Response};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
enum ChatRequest {
    Send { target: String, message: String },
    History,
}

#[derive(Debug, Serialize, Deserialize)]
enum ChatResponse {
    Ack,
    History { messages: MessageArchive },
}

type MessageArchive = Vec<(String, String)>;

fn handle_message (
    our: &Address,
    message_archive: &mut MessageArchive,
) -> anyhow::Result<()> {
    let message = await_message().unwrap();

    match message {
        Message::Response { .. } => {
            println!("{package_name}: unexpected Response: {:?}", message);
            panic!("");
        },
        Message::Request { ref source, ref ipc, .. } => {
            match serde_json::from_slice(ipc)? {
                ChatRequest::Send { ref target, ref message } => {
                    if target == &our.node {
                        println!("{package_name}|{}: {}", source.node, message);
                        message_archive.push((source.node.clone(), message.clone()));
                    } else {
                        let _ = Request::new()
                            .target(Address {
                                node: target.clone(),
                                process: ProcessId::from_str("{package_name}:{package_name}:{publisher}")?,
                            })
                            .ipc(ipc.clone())
                            .send_and_await_response(5)?
                            .unwrap();
                    }
                    Response::new()
                        .ipc(serde_json::to_vec(&ChatResponse::Ack).unwrap())
                        .send()
                        .unwrap();
                },
                ChatRequest::History => {
                    Response::new()
                        .ipc(serde_json::to_vec(&ChatResponse::History {
                            messages: message_archive.clone(),
                        }).unwrap())
                        .send()
                        .unwrap();
                },
            }
        },
    }
    Ok(())
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        println!("{package_name}: begin");

        let our = Address::from_str(&our).unwrap();
        let mut message_archive: MessageArchive = Vec::new();

        loop {
            match handle_message(&our, &mut message_archive) {
                Ok(()) => {},
                Err(e) => {
                    println!("{package_name}: error: {:?}", e);
                },
            };
        }
    }
}
