use serde::{Deserialize, Serialize};
use std::str::FromStr;

use kinode_process_lib::{await_message, call_init, println, Address, ProcessId, Request, Response};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process",
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

    if !message.is_request() {
        return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
    }

    let body = message.body();
    let source = message.source();
    match serde_json::from_slice(body)? {
        ChatRequest::Send {
            ref target,
            ref message,
        } => {
            if target == &our.node {
                println!("{}: {}", source.node, message);
                message_archive.push((source.node.clone(), message.clone()));
            } else {
                let _ = Request::new()
                    .target(Address {
                        node: target.clone(),
                        process: ProcessId::from_str(
                            "{package_name}:{package_name}:{publisher}",
                        )?,
                    })
                    .body(body)
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
    Ok(())
}

call_init!(init);
fn init(our: Address) {
    println!("begin");

    let mut message_archive: MessageArchive = Vec::new();

    loop {
        match handle_message(&our, &mut message_archive) {
            Ok(()) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        };
    }
}
