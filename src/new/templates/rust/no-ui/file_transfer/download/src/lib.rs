use crate::kinode::process::{package_name}::{Address as WitAddress, Request as TransferRequest, DownloadRequest};
use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, Message, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name_kebab}-{publisher_dotted_kebab}-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_message_body() else {
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
    let target: WitAddress = serde_json::from_str(&serde_json::to_string(&target).unwrap()).unwrap();

    let Ok(Ok(Message::Response { .. })) =
        Request::to(our)
            .body(serde_json::to_vec(&TransferRequest::Download(DownloadRequest {
                name: name.into(),
                target: target.clone(),
            })).unwrap())
            .send_and_await_response(5)
    else {
        println!("did not receive expected Response from {target:?}");
        return;
    };
}
