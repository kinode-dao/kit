use std::fs;
use std::io::{self, Read};

#[allow(deprecated)]
use base64::encode;
use reqwest;
use serde_json::{Value, json};

pub struct Response {
    pub body: String,
    pub lazy_load_blob: Option<Vec<u8>>,
}

pub fn make_message(
    process: &str,
    body: &str,
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
        "body": body,
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
    let endpoint = "/rpc:sys:nectar/message";
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

        let body = data
            .get("body")
            .map(|body| {
                if let serde_json::Value::Array(body_bytes_val) = body {
                    let body_bytes: Vec<u8> = body_bytes_val
                        .iter()
                        .map(|n| n.as_u64().unwrap() as u8)
                        .collect();
                    let body_string: String = String::from_utf8(body_bytes)?;
                    Ok(body_string)
                } else {
                    return Err(anyhow::anyhow!("Response `body` was not bytes."))
                }
            })
            .ok_or_else(|| anyhow::anyhow!("Response did not contain `body` field."))??;

        let blob = data
            .get("lazy_load_blob")
            .and_then(|b| {
                match b {
                    serde_json::Value::Null => None,
                    serde_json::Value::Array(blob_bytes_val) => {
                        let blob_bytes: Vec<u8> = blob_bytes_val
                            .iter()
                            .map(|n| n.as_u64().unwrap() as u8)
                            .collect();
                        Some(Ok(blob_bytes))
                    },
                    _ => return Some(Err(anyhow::anyhow!("Response did not contain `lazy_load_blob` bytes field."))),
                }
            })
            .transpose()?;

        Ok(Response {
            body,
            lazy_load_blob: blob,
        })
    }
}

pub async fn execute(
    url: &str,
    process: &str,
    body: &str,
    node: Option<&str>,
    bytes_path: Option<&str>,
) -> anyhow::Result<()> {
    let request = make_message(process, body, node, None, bytes_path)?;
    let response = send_request(url, request).await?;

    if response.status() == 200 {
        let content: String = response.text().await?;
        let mut data: Option<Value> = serde_json::from_str(&content).ok();

        if let Some(ref mut data_map) = data {
            if let Some(body_str) = data_map["body"].as_str() {
                let body_json: Value = serde_json::from_str(body_str).unwrap_or(Value::Null);
                data_map["body"] = body_json;
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
