use std::collections::HashMap;

use anyhow::{self};
use serde::{Deserialize, Serialize};
use uqbar_process_lib::{
    await_message, get_payload,
    http::{
        bind_http_path, send_response, send_ws_push,
        serve_ui, HttpServerRequest, IncomingHttpRequest, StatusCode, WsMessageType, bind_ws_path,
    },
    print_to_terminal, Address, Message, Payload, ProcessId, Request, Response,
};

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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    author: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NewMessage {
    chat: String,
    author: String,
    content: String,
}

type MessageArchive = HashMap<String, Vec<ChatMessage>>;

fn handle_http_server_request(
    our: &Address,
    message_archive: &mut MessageArchive,
    our_channel_id: &mut u32,
    source: &Address,
    ipc: &[u8],
) -> anyhow::Result<()> {
    let Ok(server_request) = serde_json::from_slice::<HttpServerRequest>(ipc) else {
        // Fail silently if we can't parse the request
        return Ok(());
    };

    match server_request {
        HttpServerRequest::WebSocketOpen { channel_id, .. } => {
            // Set our channel_id to the newly opened channel
            // Note: this code could be improved to support multiple channels
            *our_channel_id = channel_id;
        }
        HttpServerRequest::WebSocketPush { .. } => {
            let Some(payload) = get_payload() else {
                return Ok(());
            };

            handle_chat_request(
                our,
                message_archive,
                our_channel_id,
                source,
                &payload.bytes,
                false,
            )?;
        }
        HttpServerRequest::WebSocketClose(_channel_id) => {}
        HttpServerRequest::Http(IncomingHttpRequest { method, .. }) => {
            match method.as_str() {
                // Get all messages
                "GET" => {
                    let mut headers = HashMap::new();
                    headers.insert("Content-Type".to_string(), "application/json".to_string());

                    send_response(
                        StatusCode::OK,
                        Some(headers),
                        serde_json::to_vec(&ChatResponse::History {
                            messages: message_archive.clone(),
                        })
                        .unwrap(),
                    )?;
                }
                // Send a message
                "POST" => {
                    let Some(payload) = get_payload() else {
                        return Ok(());
                    };
                    handle_chat_request(
                        our,
                        message_archive,
                        our_channel_id,
                        source,
                        &payload.bytes,
                        true,
                    )?;

                    // Send an http response via the http server
                    send_response(StatusCode::CREATED, None, vec![])?;
                }
                _ => {
                    // Method not allowed
                    send_response(StatusCode::METHOD_NOT_ALLOWED, None, vec![])?;
                }
            }
        }
    };

    Ok(())
}

fn handle_chat_request(
    our: &Address,
    message_archive: &mut MessageArchive,
    channel_id: &mut u32,
    source: &Address,
    ipc: &[u8],
    is_http: bool,
) -> anyhow::Result<()> {
    let Ok(chat_request) = serde_json::from_slice::<ChatRequest>(ipc) else {
        // Fail silently if we can't parse the request
        return Ok(());
    };

    match chat_request {
        ChatRequest::Send {
            ref target,
            ref message,
        } => {
            // counterparty will be the other node in the chat with us
            let (counterparty, author) = if target == &our.node {
                (&source.node, source.node.clone())
            } else {
                (target, our.node.clone())
            };

            // If the target is not us, send a request to the target

            if target != &our.node {
                print_to_terminal(0, &format!("new message from {}: {}", source.node, message));

                match Request::new()
                    .target(Address {
                        node: target.clone(),
                        process: ProcessId::from_str("{package_name}:{package_name}:{publisher}")?,
                    })
                    .ipc(ipc)
                    .send_and_await_response(5) {
                        Ok(_) => {}
                        Err(e) => {
                            print_to_terminal(0, format!("testing: send request error: {:?}", e,).as_str());
                            return Ok(());
                        }
                    };
            }

            // Retreive the message archive for the counterparty, or create a new one if it doesn't exist
            let messages = match message_archive.get_mut(counterparty) {
                Some(messages) => messages,
                None => {
                    message_archive.insert(counterparty.clone(), Vec::new());
                    message_archive.get_mut(counterparty).unwrap()
                }
            };

            let new_message = ChatMessage {
                author: author.clone(),
                content: message.clone(),
            };

            // If this is an HTTP request, handle the response in the calling function
            if is_http {
                // Add the new message to the archive
                messages.push(new_message);
                return Ok(());
            }

            // If this is not an HTTP request, send a response to the other node
            Response::new()
                .ipc(serde_json::to_vec(&ChatResponse::Ack).unwrap())
                .send()
                .unwrap();

            // Add the new message to the archive
            messages.push(new_message);

            // Generate a Payload for the new message
            let payload = Payload {
                mime: Some("application/json".to_string()),
                bytes: serde_json::json!({
                    "NewMessage": NewMessage {
                        chat: counterparty.clone(),
                        author,
                        content: message.clone(),
                    }
                })
                .to_string()
                .as_bytes()
                .to_vec(),
            };

            // Send a WebSocket message to the http server in order to update the UI
            send_ws_push(
                our.node.clone(),
                channel_id.clone(),
                WsMessageType::Text,
                payload,
            )?;
        }
        ChatRequest::History => {
            // If this is an HTTP request, send a response to the http server

            Response::new()
                .ipc(
                    serde_json::to_vec(&ChatResponse::History {
                        messages: message_archive.clone(),
                    })
                    .unwrap(),
                )
                .send()
                .unwrap();
        }
    };

    Ok(())
}

fn handle_message(
    our: &Address,
    message_archive: &mut MessageArchive,
    channel_id: &mut u32,
) -> anyhow::Result<()> {
    let message = await_message().unwrap();

    match message {
        Message::Response { .. } => {
            print_to_terminal(0, &format!("{package_name}: got response - {:?}", message));
            return Ok(());
        }
        Message::Request {
            ref source,
            ref ipc,
            ..
        } => {
            // Requests that come from other nodes running this app
            handle_chat_request(our, message_archive, channel_id, source, &ipc, false)?;
            // Requests that come from our http server
            handle_http_server_request(our, message_archive, channel_id, source, ipc)?;
        }
    }

    Ok(())
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        print_to_terminal(0, "{package_name}: begin");

        let our = Address::from_str(&our).unwrap();
        let mut message_archive: MessageArchive = HashMap::new();
        let mut channel_id = 0;

        // Bind UI files to routes; index.html is bound to "/"
        serve_ui(&our, "ui").unwrap();

        // Bind HTTP path /messages
        bind_http_path("/messages", true, false).unwrap();

        // Bind WebSocket path
        bind_ws_path("/", true, false).unwrap();

        loop {
            match handle_message(&our, &mut message_archive, &mut channel_id) {
                Ok(()) => {}
                Err(e) => {
                    print_to_terminal(0, format!("{package_name}: error: {:?}", e).as_str());
                }
            };
        }
    }
}
