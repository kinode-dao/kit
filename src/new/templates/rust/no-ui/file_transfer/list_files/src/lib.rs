use serde::{Deserialize, Serialize};

use kinode_process_lib::{
    await_next_request_body, call_init, println, Address, Message, Request,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
});

#[derive(Serialize, Deserialize, Debug)]
pub enum TransferRequest {
    ListFiles,
    Download { name: String, target: Address },
    Progress { name: String, progress: u64 },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TransferResponse {
    ListFiles(Vec<FileInfo>),
    Download { name: String, worker: Address },
    Done,
    Started,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
}

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_request_body() else {
        println!("failed to get args!");
        return;
    };

    let who = String::from_utf8(body).unwrap_or_default();
    if who.is_empty() {
        println!("usage: {}@list_files:{package_name}:{publisher} who", our);
        return;
    }

    let target: Address = format!("{}@{package_name}:{package_name}:{publisher}", who)
        .parse()
        .unwrap();

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to(target)
            .body(serde_json::to_vec(&TransferRequest::ListFiles).unwrap())
            .send_and_await_response(5)
    else {
        println!("did not receive expected Response from {who}");
        return;
    };

    let Ok(TransferResponse::ListFiles(files)) = serde_json::from_slice(&body) else {
        println!("did not receive expected ListFiles from {who}");
        return;
    };

    println!(
        "{}",
        files.iter().
            fold(format!("{who} available files:\nFile\t\tSize (bytes)\n"), |mut msg, file| {
                msg.push_str(&format!(
                    "{}\t\t{}", file.name.split('/').last().unwrap(),
                    file.size,
                ));
                msg
            })
    );
}
