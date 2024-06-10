use crate::kinode::process::standard::{ProcessId as WitProcessId};
use crate::kinode::process::{package_name}::{Address as WitAddress, Request as TransferRequest, Response as TransferResponse, WorkerRequest, DownloadRequest, ProgressRequest, FileInfo, InitializeRequest};
use crate::kinode::process::tester::{Request as TesterRequest, Response as TesterResponse, RunRequest, FailResponse};

use kinode_process_lib::{await_message, call_init, print_to_terminal, println, Address, ProcessId, Request, Response};

mod tester_lib;

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name_kebab}-test-{publisher_dotted_kebab}-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

fn test_list_files(address: &Address) -> anyhow::Result<Vec<FileInfo>> {
    let response = Request::new()
        .target(address)
        .body(TransferRequest::ListFiles)
        .send_and_await_response(15)?.unwrap();
    if response.is_request() { fail!("{package_name}_test"); };
    let TransferResponse::ListFiles(files) = response.body().try_into()? else {
        fail!("{package_name}_test");
    };
    Ok(files)
}

fn test_download(name: String, our: &Address, address: &Address) -> anyhow::Result<()> {
    let response = Request::new()
        .target(our)
        .body(TransferRequest::Download(DownloadRequest {
            name,
            target: address,
        }))
        .send_and_await_response(15)?.unwrap();
    if response.is_request() { fail!("{package_name}_test"); };
    let TransferResponse::Download = response.body().try_into()? else {
        fail!("{package_name}_test");
    };
    Ok(())
}

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
    assert!(node_names.len() >= 2);
    if our.node != node_names[0] {
        // we are not master node: return
        Response::new()
            .body(TesterResponse::Run(Ok(())))
            .send()
            .unwrap();
        return Ok(());
    }

    // we are master node

    let our_ft_address = Address {
        node: our.node.clone(),
        process: ProcessId::new(Some("{package_name}"), "{package_name}", "{publisher}"),
    };
    let their_ft_address = Address {
        node: node_names[1].clone(),
        process: ProcessId::new(Some("{package_name}"), "{package_name}", "{publisher}"),
    };

    // Send
    print_to_terminal(0, "{package_name}_test: b");
    let message: String = "hello".into();
    let _ = Request::new()
        .target(our_ft_address.clone())
        .body(ChatRequest::Send(SendRequest {
            target: node_names[1].clone(),
            message: message.clone(),
        }))
        .send_and_await_response(15)?.unwrap();

    // Get history from receiver & test
    print_to_terminal(0, "{package_name}_test: c");
    let response = Request::new()
        .target(their_ft_address.clone())
        .body(ChatRequest::History(our.node.clone()))
        .send_and_await_response(15)?.unwrap();
    if response.is_request() { fail!("{package_name}_test"); };
    let ChatResponse::History(messages) = response.body().try_into()? else {
        fail!("{package_name}_test");
    };
    let expected_messages = vec![ChatMessage {
        author: our.node.clone(),
        content: message,
    }];

    if messages != expected_messages {
        println!("{messages:?} != {expected_messages:?}");
        fail!("{package_name}_test");
    }

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
