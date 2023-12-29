use std::fs;
use std::io::{self, Read};

#[allow(deprecated)]
use base64::encode;
use reqwest;
use serde_json::{Value, json};

pub async fn send_request(
    url: &str,
    json_data: Value,
) -> anyhow::Result<reqwest::Response> {
    let endpoint = "/rpc:sys:uqbar/message";
    let mut url = url.to_string();
    let url =
        if url.ends_with(endpoint) {
            url
        } else {
            if url.ends_with('/') {
                url.pop();
            }
            format!("{}{}", url, endpoint)
        };
    let client = reqwest::Client::new();
    let response = client.post(&url)
        .json(&json_data)
        .send()
        .await?;

    Ok(response)
}

pub fn make_message(
    process: &str,
    ipc: &str,
    node: Option<&str>,
    raw_bytes: Option<&[u8]>,
    bytes_path: Option<&str>,
) -> io::Result<Value> {
    #[allow(deprecated)]
    let data = match (raw_bytes, bytes_path) {
        (Some(bytes), None) => Some(encode(bytes)),
        (None, Some(path)) => {
            let mut file = fs::File::open(path)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            Some(encode(&buffer))
        },
        (None, None) => None,
        _ => {
            println!("Cannot accept both raw_bytes and bytes_path");
            std::process::exit(1);
        }
    };

    let request = json!({
        "node": node,
        "process": process,
        "inherit": false,
        "expects_response": Option::<bool>::None,
        "ipc": ipc,
        "metadata": Option::<serde_json::Value>::None,
        "context": Option::<serde_json::Value>::None,
        "mime": "application/octet-stream",
        "data": data
    });

    Ok(request)
}

pub async fn execute(
    url: &str,
    process: &str,
    ipc: &str,
    node: Option<&str>,
    bytes_path: Option<&str>,
) -> anyhow::Result<()> {
    let request = make_message(process, ipc, node, None, bytes_path)?;
    let response = send_request(url, request).await?;

    if response.status() == 200 {
        let content: String = response.text().await?;
        let mut data: Option<Value> = serde_json::from_str(&content).ok();

        if let Some(ref mut data_map) = data {
            if let Some(ipc_str) = data_map["ipc"].as_str() {
                let ipc_json: Value = serde_json::from_str(ipc_str).unwrap_or(Value::Null);
                data_map["ipc"] = ipc_json;
            }

            if let Some(payload_str) = data_map["payload"].as_str() {
                let payload_json: Value = serde_json::from_str(payload_str).unwrap_or(Value::Null);
                data_map["bytes"] = payload_json;
            }
        }
        println!("{:?}", content);
    } else {
        println!("Failed with status code: {}", response.status());
    }

    Ok(())
}
