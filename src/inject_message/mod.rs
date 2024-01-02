use std::fs;
use std::io::{self, Read};

#[allow(deprecated)]
use base64::encode;
use reqwest;
use serde_json::{Value, json};

pub struct Response {
    pub ipc: String,
    pub payload: Option<Vec<u8>>,
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

pub async fn parse_response(response: reqwest::Response) -> anyhow::Result<Response> {
    if response.status() != 200 {
        println!("Failed with status code: {}", response.status());
        return Err(anyhow::anyhow!("Failed with status code: {}", response.status()))
    } else {
        let content: String = response.text().await?;
        let data: Value = serde_json::from_str(&content)?;

        let ipc = data
            .get("ipc")
            .map(|ipc| {
                if let serde_json::Value::Array(ipc_bytes_val) = ipc {
                    let ipc_bytes: Vec<u8> = ipc_bytes_val
                        .iter()
                        .map(|n| n.as_u64().unwrap() as u8)
                        .collect();
                    let ipc_string: String = String::from_utf8(ipc_bytes)?;
                    Ok(ipc_string)
                } else {
                    return Err(anyhow::anyhow!("Response `ipc` was not bytes."))
                }
            })
            .ok_or_else(|| anyhow::anyhow!("Response did not contain `ipc` field."))??;

        let payload = data
            .get("payload")
            .and_then(|p| {
                match p {
                    serde_json::Value::Null => None,
                    serde_json::Value::Array(payload_bytes_val) => {
                        let payload_bytes: Vec<u8> = payload_bytes_val
                            .iter()
                            .map(|n| n.as_u64().unwrap() as u8)
                            .collect();
                        Some(Ok(payload_bytes))
                    },
                    _ => return Some(Err(anyhow::anyhow!("Response did not contain `payload` bytes field."))),
                }
            })
            .transpose()?;

        Ok(Response {
            ipc,
            payload,
        })
    }
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
