use std::io::Read;

#[allow(deprecated)]
use base64::{decode, encode};
use color_eyre::{eyre::eyre, Result};
use fs_err as fs;
use serde_json::{json, Value};
use tracing::{debug, info, instrument};

pub struct Response {
    pub body: String,
    pub lazy_load_blob_utf8: Option<Option<String>>,
    pub lazy_load_blob: Option<Vec<u8>>,
}

const ENDPOINT: &str = "/rpc:distro:sys/message";

impl std::fmt::Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(Some(ref s)) = self.lazy_load_blob_utf8 {
            write!(f, "Response:\nbody: {}\nblob: {}", self.body, s,)
        } else {
            write!(
                f,
                "Response:\nbody: {}\nblob: {:?}",
                self.body, self.lazy_load_blob,
            )
        }
    }
}

#[instrument(level = "trace", skip_all)]
pub fn make_message(
    process: &str,
    expects_response: Option<u64>,
    body: &str,
    node: Option<&str>,
    raw_bytes: Option<&[u8]>,
    bytes_path: Option<&str>,
) -> Result<Value> {
    #[allow(deprecated)]
    let data = match (raw_bytes, bytes_path) {
        (Some(bytes), None) => Some(encode(bytes)),
        (None, Some(path)) => {
            let mut file = fs::File::open(path)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            Some(encode(&buffer))
        }
        (None, None) => None,
        _ => {
            return Err(eyre!("Cannot accept both raw_bytes and bytes_path"));
        }
    };

    let request = json!({
        "node": node,
        "process": process,
        "inherit": false,
        "expects_response": expects_response,
        "body": body,
        "metadata": Option::<serde_json::Value>::None,
        "context": Option::<serde_json::Value>::None,
        "mime": "application/octet-stream",
        "data": data
    });

    Ok(request)
}

#[instrument(level = "trace", skip_all)]
pub async fn send_request(url: &str, json_data: Value) -> Result<reqwest::Response> {
    send_request_inner(url, json_data).await
}

/// send_request_inner() allows failure without logging;
///  used for run_tests where nodes are pinged until they
///  respond with a 200 to determine when they are online
pub async fn send_request_inner(url: &str, json_data: Value) -> Result<reqwest::Response> {
    let mut url = url.to_string();
    let url = if url.ends_with(ENDPOINT) {
        url
    } else {
        if url.ends_with('/') {
            url.pop();
        }
        format!("{}{}", url, ENDPOINT)
    };
    let client = reqwest::Client::new();
    let response = client.post(&url).json(&json_data).send().await?;

    Ok(response)
}

#[instrument(level = "trace", skip_all)]
pub async fn parse_response(response: reqwest::Response) -> Result<Response> {
    if response.status() != 200 {
        let response_status = response.status();
        let response_text = response.text().await.unwrap_or_default();

        debug!(
            "Failed with status code: {}\nResponse: {}",
            response_status, response_text,
        );
        return Err(eyre!("Failed with status code: {}", response_status));
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
                    return Err(eyre!("Response `body` was not bytes."));
                }
            })
            .ok_or_else(|| eyre!("Response did not contain `body` field."))??;

        #[allow(deprecated)]
        let blob = data
            .get("lazy_load_blob")
            .and_then(|b| match b {
                serde_json::Value::Null => None,
                serde_json::Value::Array(blob_bytes_val) => {
                    let blob_bytes: Vec<u8> = blob_bytes_val
                        .iter()
                        .map(|n| n.as_u64().unwrap() as u8)
                        .collect();
                    Some(Ok(blob_bytes))
                }
                serde_json::Value::Object(blob_object) => blob_object.get("bytes").and_then(|bb| {
                    let serde_json::Value::Array(blob_bytes_val) = bb else {
                        return Some(Err(eyre!("Unexpected `lazy_load_blob` format: {:?}.", b,)));
                    };
                    let blob_bytes: Vec<u8> = blob_bytes_val
                        .iter()
                        .map(|n| n.as_u64().unwrap() as u8)
                        .collect();
                    Some(Ok(blob_bytes))
                }),
                _ => {
                    return Some(Err(eyre!(
                        "Response did not contain `lazy_load_blob` bytes field.",
                    )))
                }
            })
            .transpose()?
            .and_then(|b| decode(b).ok());

        Ok(Response {
            body,
            lazy_load_blob_utf8: blob.clone().map(|b| String::from_utf8(b).ok()),
            lazy_load_blob: blob,
        })
    }
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    url: &str,
    process: &str,
    expects_response: Option<u64>,
    body: &str,
    node: Option<&str>,
    bytes_path: Option<&str>,
) -> Result<()> {
    let request = make_message(process, expects_response, body, node, None, bytes_path)?;
    let response = send_request(url, request).await?;
    if expects_response.is_some() {
        let response = parse_response(response).await?;
        info!("{}", response);
    } else {
        if response.status() != 200 {
            return Err(eyre!("Failed with status code: {}", response.status()));
        } else {
            info!("{}", response.status());
        }
    }

    Ok(())
}
