use kinode_process_lib::{await_message, call_init, println, Address, Message, Response};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

fn handle_message(_our: &Address) -> anyhow::Result<()> {
    let message = await_message()?;

    match message {
        Message::Response { .. } => {
            return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
        }
        Message::Request {
            ref body,
            ..
        } => {
            let body: serde_json::Value = serde_json::from_slice(body)?;
            println!("{package_name}: got {body:?}");
            Response::new()
                .body(serde_json::to_vec(&serde_json::json!("Ack")).unwrap())
                .send()
                .unwrap();
        }
    }
    Ok(())
}

call_init!(init);

fn init(our: Address) {
    println!("{package_name}: begin");

    loop {
        match handle_message(&our) {
            Ok(()) => {}
            Err(e) => {
                println!("{package_name}: error: {:?}", e);
            }
        };
    }
}

