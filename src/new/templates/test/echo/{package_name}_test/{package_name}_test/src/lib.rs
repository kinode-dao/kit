use crate::kinode::process::tester::{Request as TesterRequest, Response as TesterResponse, RunRequest, FailResponse};

use kinode_process_lib::{await_message, call_init, print_to_terminal, println, Address, ProcessId, Request, Response};

mod tester_lib;

wit_bindgen::generate!({
    path: "target/wit",
    world: "tester-sys-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

fn handle_message (our: &Address) -> anyhow::Result<()> {
    let message = await_message().unwrap();

    if !message.is_request() {
        unimplemented!();
    }
    let source = message.source();
    if our.node != source.node {
        return Err(anyhow::anyhow!(
            "rejecting foreign Message from {:?}",
            source,
        ));
    }
    let TesterRequest::Run(RunRequest {
        input_node_names: node_names,
        ..
    }) = message.body().try_into()?;
    print_to_terminal(0, "{package_name}_test: a");
    assert!(node_names.len() == 1);

    let our_echo_address = Address {
        node: our.node.clone(),
        process: ProcessId::new(Some("{package_name}"), "{package_name}", "{publisher}"),
    };

    // Send
    print_to_terminal(0, "{package_name}_test: b");
    let message: String = "hello".into();
    let response = Request::new()
        .target(our_echo_address)
        .body(serde_json::to_vec("test")?)
        .send_and_await_response(15)?.unwrap();
    if response.is_request() { fail!("{package_name}_test"); };
    if serde_json::json!("Ack") != serde_json::from_slice::<serde_json::Value>(
        response.body()
    ) {
        fail!("{package_name}_test");
    };

    Response::new()
        .body(TesterResponse::Run(Ok(())))
        .send()
        .unwrap();

    Ok(())
}

call_init!(init);
fn init(our: Address) {
    print_to_terminal(0, "begin");

    loop {
        match handle_message(&our) {
            Ok(()) => {},
            Err(e) => {
                print_to_terminal(0, format!("{package_name}_test: error: {e:?}").as_str());

                fail!("{package_name}_test");
            },
        };
    }
}
