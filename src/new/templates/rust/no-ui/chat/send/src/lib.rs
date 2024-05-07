use serde::{Deserialize, Serialize};

use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, Message, Request,
};

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

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_message_body() else {
        println!("failed to get args!");
        return;
    };

    let args = String::from_utf8(body).unwrap_or_default();

    let Some((target, message)) = args.split_once(" ") else {
        println!("usage:\nsend:{package_name}:{publisher} target message");
        return;
    };

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to((our.node(), ("{package_name}", "{package_name}", "{publisher}")))
            .body(
                serde_json::to_vec(&ChatRequest::Send {
                    target: target.into(),
                    message: message.into(),
                })
                .unwrap(),
            )
            .send_and_await_response(5)
    else {
        println!("did not receive expected Response from {package_name}:{package_name}:{publisher}");
        return;
    };

    let Ok(ChatResponse::Ack) = serde_json::from_slice(&body) else {
        println!("did not receive expected Ack from {package_name}:{package_name}:{publisher}");
        return;
    };
}
