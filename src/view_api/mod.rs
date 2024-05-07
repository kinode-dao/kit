use std::io::{Read, Write};
use std::path::Path;

use color_eyre::{Result, eyre::{eyre, WrapErr}};
use fs_err as fs;
use serde_json::json;
use tracing::{info, instrument};
use walkdir::WalkDir;
use zip::write::FileOptions;

use kinode_process_lib::kernel_types::Erc721Metadata;

use crate::{build::read_metadata, inject_message, KIT_LOG_PATH_DEFAULT};

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

//#[instrument(level = "trace", skip_all)]
//fn zip_directory(directory: &Path, zip_filename: &str) -> Result<()> {
//    let file = fs::File::create(zip_filename)?;
//    let walkdir = WalkDir::new(directory);
//    let it = walkdir.into_iter();
//
//    let mut zip = zip::ZipWriter::new(file);
//
//    let options = FileOptions::default()
//        .compression_method(zip::CompressionMethod::Stored)
//        .unix_permissions(0o755);
//
//    for entry in it {
//        let entry = entry?;
//        let path = entry.path();
//        let name = path.strip_prefix(Path::new(directory))?;
//
//        if path.is_file() {
//            zip.start_file(name.to_string_lossy(), options)?;
//            let mut f = fs::File::open(path)?;
//            let mut buffer = Vec::new();
//            f.read_to_end(&mut buffer)?;
//            zip.write_all(&*buffer)?;
//        } else if name.as_os_str().len() != 0 {
//            // Only if it is not the root directory
//            zip.add_directory(name.to_string_lossy(), options)?;
//        }
//    }
//
//    zip.finish()?;
//    Ok(())
//}

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    node: Option<&str>,
    package_id: Option<&str>,
    url: &str,
) -> Result<()> {
    let request = if let Some(package_id) = package_id {
        let mut pids = package_id.splitn(2, ':');
        let (Some(package_name), Some(publisher_node), None) = (
            pids.next(),
            pids.next(),
            pids.next(),
        ) else {
            return Err(eyre!("package_id must be None or Some(<package>:<publisher>)"));
        };
        get_api(node, package_name, publisher_node)?
    } else {
        list_apis(node)?
    };
    let response = inject_message::send_request(url, request).await?;

    let inject_message::Response { ref body, .. } =
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
    info!("{}", serde_json::to_string_pretty(&body)?);

    Ok(())
}
