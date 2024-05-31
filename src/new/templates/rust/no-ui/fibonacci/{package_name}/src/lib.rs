use crate::kinode::process::{package_name}::{Request as FibonacciRequest, Response as FibonacciResponse};
use kinode_process_lib::{await_message, call_init, println, Address, Response};

wit_bindgen::generate!({
    path: "target/wit",
    world: "{package_name_kebab}-{publisher_dotted_kebab}-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

/// calculate the nth Fibonacci number
/// since we are using u64, the maximum number
/// we can calculate is the 93rd Fibonacci number
fn fibonacci(n: u32) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut a = 0;
    let mut b = 1;
    let mut sum;
    for _ in 1..n {
        sum = a + b;
        a = b;
        b = sum;
    }
    b
}

fn handle_message() -> anyhow::Result<()> {
    let message = await_message()?;

    if !message.is_request() {
        return Err(anyhow::anyhow!("expected a request"));
    }

    match serde_json::from_slice(message.body())? {
        FibonacciRequest::Number(number) => {
            let start = std::time::Instant::now();
            let result = fibonacci(number);
            let duration = start.elapsed();
            println!(
                "fibonacci({}) = {}; {}ns",
                number,
                result,
                duration.as_nanos(),
            );
            Response::new()
                .body(serde_json::to_vec(&FibonacciResponse::Number(result)).unwrap())
                .send()
                .unwrap();
        }
        FibonacciRequest::Numbers((number, number_trials)) => {
            let mut durations = Vec::new();
            for _ in 0..number_trials {
                let start = std::time::Instant::now();
                let _result = fibonacci(number);
                let duration = start.elapsed();
                durations.push(duration);
            }
            let result = fibonacci(number);
            let mean =
                durations.iter().fold(0, |sum, item| sum + item.as_nanos()) / number_trials as u128;
            let absolute_deviation = durations.iter().fold(0, |ad, item| {
                let trial = item.as_nanos();
                ad + if mean >= trial {
                    mean - trial
                } else {
                    trial - mean
                }
            }) / number_trials as u128;
            println!(
                "fibonacci({}) = {}; {}±{}ns averaged over {} trials",
                number, result, mean, absolute_deviation, number_trials,
            );
            Response::new()
                .body(
                    serde_json::to_vec(&FibonacciResponse::Numbers((result, number_trials)))
                        .unwrap(),
                )
                .send()
                .unwrap();
        }
    }
    Ok(())
}

call_init!(init);
fn init(_our: Address) {
    println!("begin");

    loop {
        match handle_message() {
            Ok(()) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        };
    }
}
