use serde::{Deserialize, Serialize};
use std::str::FromStr;

use nectar_process_lib::{await_message, call_init, println, Address, Message, ProcessId, Request, Response};

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

fn handle_message(our: &Address, message_archive: &mut MessageArchive) -> anyhow::Result<()> {
    let message = await_message()?;

    match message {
        Message::Response { .. } => {
            return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
        }
        Message::Request {
            ref source,
            ref body,
            ..
        } => match serde_json::from_slice(body)? {
            ChatRequest::Send {
                ref target,
                ref message,
            } => {
                if target == &our.node {
                    println!("{package_name}|{}: {}", source.node, message);
                    message_archive.push((source.node.clone(), message.clone()));
                } else {
                    let _ = Request::new()
                        .target(Address {
                            node: target.clone(),
                            process: ProcessId::from_str(
                                "{package_name}:{package_name}:{publisher}",
                            )?,
                        })
                        .body(body.clone())
                        .send_and_await_response(5)?
                        .unwrap();
                }
                Response::new()
                    .body(serde_json::to_vec(&ChatResponse::Ack).unwrap())
                    .send()
                    .unwrap();
            }
            ChatRequest::History => {
                Response::new()
                    .body(
                        serde_json::to_vec(&ChatResponse::History {
                            messages: message_archive.clone(),
                        })
                        .unwrap(),
                    )
                    .send()
                    .unwrap();
            }
        }
    }
    Ok(())
}

call_init!(init);

fn init(our: Address) {
    println!("{package_name}: begin");

    let mut message_archive: MessageArchive = Vec::new();

    loop {
        match handle_message(&our, &mut message_archive) {
            Ok(()) => {}
            Err(e) => {
                println!("{package_name}: error: {:?}", e);
            }
        };
    }
}

