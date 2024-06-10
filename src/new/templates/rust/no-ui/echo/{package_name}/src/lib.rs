use kinode_process_lib::{await_message, call_init, println, Address, Response};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

fn handle_message(_our: &Address) -> anyhow::Result<()> {
    let message = await_message()?;

    if !message.is_request() {
        return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
    }

    let body: serde_json::Value = serde_json::from_slice(message.body())?;
    println!("got {body:?}");
    Response::new()
        .body(serde_json::to_vec(&serde_json::json!("Ack")).unwrap())
        .send()
        .unwrap();
    Ok(())
}

call_init!(init);
fn init(our: Address) {
    println!("begin");

    loop {
        match handle_message(&our) {
            Ok(()) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        };
    }
}
