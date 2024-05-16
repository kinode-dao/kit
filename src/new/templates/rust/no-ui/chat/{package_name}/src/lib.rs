use std::collections::HashMap;
use std::str::FromStr;

use crate::kinode::process::{package_name}::{ChatMessage, Request as ChatRequest, Response as ChatResponse, SendRequest};
use kinode_process_lib::{await_message, call_init, println, Address, ProcessId, Request, Response};

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name}-{publisher_dotted_kebab}-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

type MessageArchive = HashMap<String, Vec<ChatMessage>>;

fn handle_message(our: &Address, message_archive: &mut MessageArchive) -> anyhow::Result<()> {
    let message = await_message()?;

    if !message.is_request() {
        return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
    }

    let body = message.body();
    let source = message.source();
    match serde_json::from_slice(body)? {
        ChatRequest::Send(SendRequest {
            ref target,
            ref message,
        }) => {
            if target == &our.node {
                println!("{}: {}", source.node, message);
                let message = ChatMessage {
                    author: source.node.clone(),
                    content: message.into(),
                };
                message_archive
                    .entry(source.node.clone())
                    .and_modify(|e| e.push(message.clone()))
                    .or_insert(vec![message]);
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
                let message = ChatMessage {
                    author: our.node.clone(),
                    content: message.into(),
                };
                message_archive
                    .entry(target.clone())
                    .and_modify(|e| e.push(message.clone()))
                    .or_insert(vec![message]);
            }
            Response::new()
                .body(serde_json::to_vec(&ChatResponse::Send).unwrap())
                .send()
                .unwrap();
        }
        ChatRequest::History(ref node) => {
            Response::new()
                .body(serde_json::to_vec(&ChatResponse::History(
                    message_archive
                        .get(node)
                        .map(|msgs| msgs.clone())
                        .unwrap_or_default()
                )).unwrap())
                .send()
                .unwrap();
        }
    }
    Ok(())
}

call_init!(init);
fn init(our: Address) {
    println!("begin");

    let mut message_archive = HashMap::new();

    loop {
        match handle_message(&our, &mut message_archive) {
            Ok(()) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        };
    }
}
