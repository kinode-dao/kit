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
fn new_package(
    node: Option<&str>,
    package_name: &str,
    publisher_node: &str,
    metadata: &Erc721Metadata,
    bytes_path: &str,
) -> Result<serde_json::Value> {
    let message = json!({
        "NewPackage": {
            "package": {"package_name": package_name, "publisher_node": publisher_node},
            "metadata": metadata,
            "mirror": true
        }
    });

    inject_message::make_message(
        "main:app_store:sys",
        Some(15),
        &message.to_string(),
        node,
        None,
        Some(bytes_path),
    )
}

#[instrument(level = "trace", skip_all)]
pub fn interact_with_package(
    request_type: &str,
    node: Option<&str>,
    package_name: &str,
    publisher_node: &str,
) -> Result<serde_json::Value> {
    let message = json!({
        request_type: {
            "package_name": package_name,
            "publisher_node": publisher_node,
        }
    });

    inject_message::make_message(
        "main:app_store:sys",
        Some(15),
        &message.to_string(),
        node,
        None,
        None,
    )
}

#[instrument(level = "trace", skip_all)]
pub fn zip_directory(directory: &Path, zip_filename: &str) -> Result<()> {
    let file = fs::File::create(zip_filename)?;
    let walkdir = WalkDir::new(directory);
    let it = walkdir.into_iter();

    let mut zip = zip::ZipWriter::new(file);

    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);

    for entry in it {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(Path::new(directory))?;

        if path.is_file() {
            zip.start_file(name.to_string_lossy(), options)?;
            let mut f = fs::File::open(path)?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;
            zip.write_all(&*buffer)?;
        } else if name.as_os_str().len() != 0 {
            // Only if it is not the root directory
            zip.add_directory(name.to_string_lossy(), options)?;
        }
    }

    zip.finish()?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(package_dir: &Path, url: &str) -> Result<()> {
    if !package_dir.join("pkg").exists() {
        return Err(eyre!(
            "Required `pkg/` dir not found within given input dir {:?} (or cwd, if none given). Please re-run targeting a package.",
            package_dir,
        ));
    }
    let pkg_dir = package_dir.join("pkg").canonicalize()?;
    let metadata = read_metadata(package_dir)?;
    let package_name = metadata.properties.package_name.as_str();
    let publisher = metadata.properties.publisher.as_str();
    let pkg_publisher = format!("{}:{}", package_name, publisher);
    info!("{}", pkg_publisher);

    // Create zip and put it in /target
    let parent_dir = pkg_dir.parent().unwrap();
    let target_dir = parent_dir.join("target");
    fs::create_dir_all(&target_dir)?;
    let zip_filename = target_dir.join(&pkg_publisher).with_extension("zip");
    zip_directory(&pkg_dir, &zip_filename.to_str().unwrap())?;

    // Create and send new package request
    let new_pkg_request = new_package(
        None,
        package_name,
        publisher,
        &metadata,
        zip_filename.to_str().unwrap(),
    )?;
    let response = inject_message::send_request(url, new_pkg_request).await?;
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
    let new_package_response = body.get("NewPackageResponse");

    if new_package_response != Some(&serde_json::Value::String("Success".to_string())) {
        return Err(eyre!("Failed to add package. Got response from node: {}", body));
    }

    // Install package
    let install_request = interact_with_package("Install", None, package_name, publisher)?;
    let response = inject_message::send_request(url, install_request).await?;
    let inject_message::Response { ref body, .. } =
        inject_message::parse_response(response).await?;
    let body = serde_json::from_str::<serde_json::Value>(body)?;
    let install_response = body.get("InstallResponse");

    if install_response == Some(&serde_json::Value::String("Success".to_string())) {
        info!("Successfully installed package {} on node at {}", pkg_publisher, url);
    } else {
        return Err(eyre!("Failed to start package. Got response from node: {}", body));
    }

    Ok(())
}
