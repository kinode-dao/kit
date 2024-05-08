use std::path::PathBuf;

use color_eyre::{Result, eyre::eyre};
use fs_err as fs;
use serde_json::json;
use tracing::{info, instrument};

use crate::{boot_fake_node::extract_zip, inject_message, KIT_CACHE, KIT_LOG_PATH_DEFAULT};

#[instrument(level = "trace", skip_all)]
fn list_apis(node: Option<&str>) -> Result<serde_json::Value> {
    let message = json!("ListApis");

    inject_message::make_message(
        "main:app_store:sys",
        Some(5),
        &message.to_string(),
        node,
        None,
        None,
    )
}

#[instrument(level = "trace", skip_all)]
fn get_api(
    node: Option<&str>,
    package_name: &str,
    publisher_node: &str,
) -> Result<serde_json::Value> {
    let message = json!({
        "GetApi": {
            "package_name": package_name,
            "publisher_node": publisher_node,
        },
    });

    inject_message::make_message(
        "main:app_store:sys",
        Some(5),
        &message.to_string(),
        node,
        None,
        None,
    )
}

#[instrument(level = "trace", skip_all)]
fn split_package_id(package_id: &str) -> Result<(String, String)> {
    let mut pids = package_id.splitn(2, ':');
    let (Some(package_name), Some(publisher_node), None) = (
        pids.next(),
        pids.next(),
        pids.next(),
    ) else {
        return Err(eyre!("package_id must be None or Some(<package>:<publisher>)"));
    };
    Ok((package_name.to_string(), publisher_node.to_string()))
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    node: Option<&str>,
    package_id: Option<&str>,
    url: &str,
    verbose: bool,
) -> Result<Option<PathBuf>> {
    let request = if let Some(package_id) = package_id {
        let (package_name, publisher_node) = split_package_id(package_id)?;
        get_api(node, &package_name, &publisher_node)?
    } else {
        list_apis(node)?
    };
    let response = inject_message::send_request(url, request).await?;

    let inject_message::Response { ref body, ref lazy_load_blob, .. } =
        inject_message::parse_response(response)
            .await
            .map_err(|e| {
                let e_string = e.to_string();
                if e_string.contains("Failed with status code:") {
                    eyre!("{}\ncheck logs (default at {}) for full http response\n\nhint: is Kinode running at url {}?", e_string, KIT_LOG_PATH_DEFAULT, url)
                } else {
                    eyre!(e_string)
                }
            })?;
    let body = serde_json::from_str::<serde_json::Value>(body)?;
    let zip_dir = if let Some(blob) = lazy_load_blob {
        let api_name = format!("{}-api", package_id.unwrap());
        let zip_dir = PathBuf::from(KIT_CACHE).join(api_name);
        let zip_path = zip_dir.join(format!("{}-api.zip", package_id.unwrap()));
        if zip_dir.exists() {
            fs::remove_dir_all(&zip_dir)?;
        }
        fs::create_dir_all(&zip_dir)?;
        fs::write(&zip_path, blob)?;
        extract_zip(&zip_path)?;
        for entry in zip_dir.read_dir()? {
            let entry = entry?;
            let path = entry.path();
            if Some("wit") == path.extension().and_then(|s| s.to_str()) {
                let file_path = path.to_str().unwrap_or_default();
                let wit_contents = fs::read_to_string(&path)?;
                if verbose {
                    info!("{}\n\n{}", file_path, wit_contents);
                }
            }
        }
        Some(zip_dir)
    } else {
        if verbose {
            info!("{}", serde_json::to_string_pretty(&body)?);
        }
        None
    };

    Ok(zip_dir)
}
