use std::path::Path;

use color_eyre::{eyre::eyre, Result, Section};
use fs_err as fs;
use serde_json::json;
use tracing::{info, instrument};

use kinode_process_lib::kernel_types::{Erc721Metadata, PackageManifestEntry};

use crate::build::{hash_zip_pkg, make_pkg_publisher, make_zip_filename, read_metadata};
use crate::{inject_message, KIT_LOG_PATH_DEFAULT};

#[instrument(level = "trace", skip_all)]
fn new_package(
    node: Option<&str>,
    package_name: &str,
    publisher_node: &str,
    bytes_path: &str,
) -> Result<serde_json::Value> {
    let message = json!({
        "NewPackage": {
            "package_id": {"package_name": package_name, "publisher_node": publisher_node},
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
fn install(
    node: Option<&str>,
    hash_string: &str,
    metadata: &Erc721Metadata,
) -> Result<serde_json::Value> {
    let body = json!({
        "Install": {
            "package_id": {
                "package_name": metadata.properties.package_name,
                "publisher_node": metadata.properties.publisher,
            },
            "version_hash": hash_string,
            "metadata": {
                "name": metadata.name,
                "description": metadata.description,
                "image": metadata.image,
                "external_url": metadata.external_url,
                "animation_url": metadata.animation_url,
                "properties": {
                    "package_name": metadata.properties.package_name,
                    "publisher": metadata.properties.publisher,
                    "current_version": metadata.properties.current_version,
                    "mirrors": metadata.properties.mirrors,
                    "code_hashes": metadata.properties.code_hashes.clone().into_iter().collect::<Vec<(String, String)>>(),
                    "license": metadata.properties.license,
                    "screenshots": metadata.properties.screenshots,
                    "wit_version": metadata.properties.wit_version,
                    "dependencies": metadata.properties.dependencies,
                },
            },
        }
    });

    inject_message::make_message(
        "main:app_store:sys",
        Some(15),
        &body.to_string(),
        node,
        None,
        None,
    )
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
    let pkg_publisher = make_pkg_publisher(&metadata);
    let zip_filename = make_zip_filename(package_dir, &pkg_publisher);

    if !zip_filename.exists() {
        return Err(eyre!("Missing pkg zip.")
            .with_suggestion(|| "Try `kit build`ing package first."));
    }

    let manifest = fs::File::open(pkg_dir.join("manifest.json"))
        .with_suggestion(|| "Missing required manifest.json file. See discussion at https://book.kinode.org/my_first_app/chapter_1.html?highlight=manifest.json#pkgmanifestjson")?;
    let manifest: Vec<PackageManifestEntry> = serde_json::from_reader(manifest)
        .with_suggestion(|| "Failed to parse required manifest.json file. See discussion at https://book.kinode.org/my_first_app/chapter_1.html?highlight=manifest.json#pkgmanifestjson")?;
    let has_all_entries = manifest.iter().fold(true, |has_all_entries, entry| {
        let file_path = entry
            .process_wasm_path
            .strip_prefix("/")
            .unwrap_or_else(|| &entry.process_wasm_path);
        has_all_entries && pkg_dir.join(file_path).exists()
    });
    if !has_all_entries {
        return Err(eyre!("Missing a .wasm file declared by manifest.json.")
            .with_suggestion(|| "Try `kit build`ing package first, or updating manifest.json."));
    }

    info!("{}", pkg_publisher);
    let hash_string = hash_zip_pkg(&zip_filename)?;

    // Create and send new package request
    let new_pkg_request = new_package(
        None,
        package_name,
        publisher,
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
        return Err(eyre!(
            "Failed to add package. Got response from node: {}",
            body
        ));
    }

    let install_request = install(
        None,
        &hash_string,
        &metadata,
    )?;
    let response = inject_message::send_request(url, install_request).await?;
    let inject_message::Response { ref body, .. } =
        inject_message::parse_response(response).await?;
    let body = serde_json::from_str::<serde_json::Value>(body)?;
    let install_response = body.get("InstallResponse");

    if install_response == Some(&serde_json::Value::String("Success".to_string())) {
        info!(
            "Successfully installed package {} on node at {}",
            pkg_publisher, url
        );
    } else {
        return Err(eyre!(
            "Failed to start package. Got response from node: {}",
            body
        ));
    }

    Ok(())
}
