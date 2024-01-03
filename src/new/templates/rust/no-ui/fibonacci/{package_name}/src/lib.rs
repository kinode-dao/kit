use serde::{Serialize, Deserialize};

use uqbar_process_lib::{await_message, print_to_terminal, Address, Message, Response};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
enum FibonacciRequest {
    Number(u32),
    Numbers((u32, u32)),
}

#[derive(Debug, Serialize, Deserialize)]
enum FibonacciResponse {
    Number(u64),
    Numbers((u64, u32)),
}

fn fibonacci(n: u32) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn handle_message (our: &Address) -> anyhow::Result<()> {
    let message = await_message().unwrap();

    match message {
        Message::Response { .. } => {
            return Err(anyhow::anyhow!("unexpected Response: {:?}", message))
        },
        Message::Request { ref source, ref ipc, .. } => {
            if source.node != our.node {
                return Err(anyhow::anyhow!("dropping foreign Request from {}", source));
            }
            match serde_json::from_slice(ipc)? {
                FibonacciRequest::Number(number) => {
                    let start = std::time::Instant::now();
                    let result = fibonacci(number);
                    let duration = start.elapsed();
                    print_to_terminal(0, &format!(
                        "{package_name}: fibonacci({}) = {}; {}ns",
                        number,
                        result,
                        duration.as_nanos(),
                    ));
                    Response::new()
                        .ipc(serde_json::to_vec(&FibonacciResponse::Number(result)).unwrap())
                        .send()
                        .unwrap();
                },
                FibonacciRequest::Numbers((number, number_trials)) => {
                    let mut durations = Vec::new();
                    for _ in 0..number_trials {
                        let start = std::time::Instant::now();
                        let _result = fibonacci(number);
                        let duration = start.elapsed();
                        durations.push(duration);
                    }
                    let result = fibonacci(number);
                    let mean = durations
                        .iter()
                        .fold(0, |sum, item| sum + item.as_nanos()) / number_trials as u128;
                    let absolute_deviation = durations
                        .iter()
                        .fold(0, |ad, item| {
                            let trial = item.as_nanos();
                            ad + if mean >= trial { mean - trial } else { trial - mean }
                        }) / number_trials as u128;
                    print_to_terminal(0, &format!(
                        "{package_name}: fibonacci({}) = {}; {}±{}ns averaged over {} trials",
                        number,
                        result,
                        mean,
                        absolute_deviation,
                        number_trials,
                    ));
                    Response::new()
                        .ipc(serde_json::to_vec(&FibonacciResponse::Numbers((
                            result,
                            number_trials,
                        ))).unwrap())
                        .send()
                        .unwrap();
                },
            }
        },
    }
    Ok(())
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        print_to_terminal(0, "{package_name}: begin");

        let our = Address::from_str(&our).unwrap();

        loop {
            match handle_message(&our) {
                Ok(()) => {},
                Err(e) => {
                    print_to_terminal(0, format!(
                        "{package_name}: error: {:?}",
                        e,
                    ).as_str());
                },
            };
        }
    }
}
