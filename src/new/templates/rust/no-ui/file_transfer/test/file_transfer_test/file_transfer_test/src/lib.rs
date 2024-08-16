use crate::kinode::process::standard::{ProcessId as WitProcessId};
use crate::kinode::process::file_transfer::{Address as WitAddress, Request as TransferRequest, Response as TransferResponse, WorkerRequest, DownloadRequest, ProgressRequest, FileInfo, InitializeRequest};
use crate::kinode::process::tester::{Request as TesterRequest, Response as TesterResponse, RunRequest, FailResponse};

use kinode_process_lib::{await_message, call_init, print_to_terminal, println, Address, ProcessId, Request, Response};

mod tester_lib;

wit_bindgen::generate!({
    path: "target/wit",
    world: "file-transfer-test-template-dot-os-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

fn test_list_files(address: &Address) -> anyhow::Result<Vec<FileInfo>> {
    let response = Request::new()
        .target(address)
        .body(TransferRequest::ListFiles)
        .send_and_await_response(15)?.unwrap();
    if response.is_request() { fail!("file_transfer_test"); };
    let TransferResponse::ListFiles(files) = response.body().try_into()? else {
        fail!("file_transfer_test");
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
    if response.is_request() { fail!("file_transfer_test"); };
    let TransferResponse::Download = response.body().try_into()? else {
        fail!("file_transfer_test");
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
    print_to_terminal(0, "file_transfer_test: a");
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
        process: ProcessId::new(Some("file_transfer"), "file_transfer", "template.os"),
    };
    let their_ft_address = Address {
        node: node_names[1].clone(),
        process: ProcessId::new(Some("file_transfer"), "file_transfer", "template.os"),
    };

    // Send
    print_to_terminal(0, "file_transfer_test: b");
    let message: String = "hello".into();
    let _ = Request::new()
        .target(our_ft_address.clone())
        .body(ChatRequest::Send(SendRequest {
            target: node_names[1].clone(),
            message: message.clone(),
        }))
        .send_and_await_response(15)?.unwrap();

    // Get history from receiver & test
    print_to_terminal(0, "file_transfer_test: c");
    let response = Request::new()
        .target(their_ft_address.clone())
        .body(ChatRequest::History(our.node.clone()))
        .send_and_await_response(15)?.unwrap();
    if response.is_request() { fail!("file_transfer_test"); };
    let ChatResponse::History(messages) = response.body().try_into()? else {
        fail!("file_transfer_test");
    };
    let expected_messages = vec![ChatMessage {
        author: our.node.clone(),
        content: message,
    }];

    if messages != expected_messages {
        println!("{messages:?} != {expected_messages:?}");
        fail!("file_transfer_test");
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
                print_to_terminal(0, format!("file_transfer_test: error: {e:?}").as_str());

                fail!("file_transfer_test");
            },
        };
    }
}
