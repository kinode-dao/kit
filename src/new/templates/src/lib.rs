use serde::{Serialize, Deserialize};

use uqbar_process_lib::{Address, ProcessId, Request, Response};
use uqbar_process_lib::uqbar::process::standard as wit;

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
    let (source, message) = wit::receive().unwrap();

    match message {
        wit::Message::Response(_) => {
            wit::print_to_terminal(0, &format!("chat: unexpected Response: {:?}", message));
            panic!("");
        },
        wit::Message::Request(wit::Request { ref ipc, .. }) => {
            match serde_json::from_slice(ipc)? {
                ChatRequest::Send { ref target, ref message } => {
                    if target == &our.node {
                        wit::print_to_terminal(0, &format!("{package_name}|{}: {}", source.node, message));
                        message_archive.push((source.node.clone(), message.clone()));
                    } else {
                        let _ = Request::new()
                            .target(wit::Address {
                                node: target.clone(),
                                process: ProcessId::from_str("{package_name}:{package_name}:template.uq")?,
                            })
                            .ipc(ipc.clone())
                            .send_and_await_response(5)
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
        wit::print_to_terminal(0, "{package_name}: begin");

        let our = Address::from_str(&our).unwrap();
        let mut message_archive: MessageArchive = Vec::new();

        loop {
            match handle_message(&our, &mut message_archive) {
                Ok(()) => {},
                Err(e) => {
                    wit::print_to_terminal(0, format!(
                        "{package_name}: error: {:?}",
                        e,
                    ).as_str());
                },
            };
        }
    }
}
