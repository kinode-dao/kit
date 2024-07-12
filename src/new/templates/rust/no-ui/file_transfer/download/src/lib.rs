use crate::kinode::process::standard::{ProcessId as WitProcessId};
use crate::kinode::process::{package_name}::{Address as WitAddress, Request as TransferRequest, DownloadRequest};
use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, Message, ProcessId, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name_kebab}-{publisher_dotted_kebab}-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

impl From<Address> for WitAddress {
    fn from(address: Address) -> Self {
        WitAddress {
            node: address.node,
            process: address.process.into(),
        }
    }
}

impl From<ProcessId> for WitProcessId {
    fn from(process: ProcessId) -> Self {
        WitProcessId {
            process_name: process.process_name,
            package_name: process.package_name,
            publisher_node: process.publisher_node,
        }
    }
}

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_message_body() else {
        println!("failed to get args!");
        return;
    };

    let args = String::from_utf8(body).unwrap_or_default();
    let Some((name, who)) = args.split_once(" ") else {
        println!("usage: download:{package_name}:{publisher} file_name who");
        return
    };
    let our: Address = format!("{}@{package_name}:{package_name}:{publisher}", our.node())
        .parse()
        .unwrap();

    let target: Address = format!("{}@{package_name}:{package_name}:{publisher}", who)
        .parse()
        .unwrap();

    let Ok(_) = Request::to(our)
        .body(TransferRequest::Download(DownloadRequest {
            name: name.into(),
            target: target.clone().into(),
        }))
        .send();
}
