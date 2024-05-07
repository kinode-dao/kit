use serde::{Deserialize, Serialize};

use kinode_process_lib::{
    await_next_request_body, call_init, println, Address, Message, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
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

    let args = String::from_utf8(body).unwrap_or_default();
    let Some((name, who)) = args.split_once(" ") else {
        println!("usage: {}@download:{package_name}:{publisher} file_name who", our.node());
        return
    };
    let our: Address = format!("{}@{package_name}:{package_name}:{publisher}", our.node())
        .parse()
        .unwrap();

    let target: Address = format!("{}@{package_name}:{package_name}:{publisher}", who)
        .parse()
        .unwrap();

    let Ok(Ok(Message::Response { .. })) =
        Request::to(our)
            .body(serde_json::to_vec(&TransferRequest::Download {
                name: name.into(),
                target: target.clone(),
            }).unwrap())
            .send_and_await_response(5)
    else {
        println!("did not receive expected Response from {target}");
        return;
    };
}
