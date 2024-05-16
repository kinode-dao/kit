use crate::kinode::process::{package_name}::{Request as FibonacciRequest, Response as FibonacciResponse};
use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, Message, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name}-{publisher_dotted_kebab}-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_message_body() else {
        println!("failed to get args!");
        return;
    };

    let number: u32 = String::from_utf8(body)
        .unwrap_or_default()
        .parse()
        .unwrap_or_default();

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to((our.node(), ("{package_name}", "{package_name}", "{publisher}")))
            .body(serde_json::to_vec(&FibonacciRequest::Number(number)).unwrap())
            .send_and_await_response(5)
    else {
        println!("did not receive expected Response from {package_name}:{package_name}:{publisher}");
        return;
    };

    let Ok(FibonacciResponse::Number(_number)) = serde_json::from_slice(&body) else {
        println!("did not receive expected Ack from {package_name}:{package_name}:{publisher}");
        return;
    };

    // don't need to print the number here since the main process prints it
}
