use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::json;
use walkdir::WalkDir;
use zip::write::FileOptions;

use super::inject_message;

fn new_package(
    node: Option<&str>,
    package_name: &str,
    publisher_node: &str,
    bytes_path: &str,
) -> io::Result<serde_json::Value> {
    let message = json!({
        "NewPackage": {
            "package": {"package_name": package_name, "publisher_node": publisher_node},
            "mirror": true
        }
    });

    inject_message::make_message(
        "main:app_store:uqbar",
        &message.to_string(),
        node,
        None,
        Some(bytes_path),
    )
}

pub fn interact_with_package(
    request_type: &str,
    node: Option<&str>,
    package_name: &str,
    publisher_node: &str,
) -> io::Result<serde_json::Value> {
    let message = json!({
        request_type: {
            "package_name": package_name,
            "publisher_node": publisher_node,
        }
    });

    inject_message::make_message(
        "main:app_store:uqbar",
        &message.to_string(),
        node,
        None,
        None,
    )
}

fn zip_directory(directory: &Path, zip_filename: &str) -> anyhow::Result<()> {
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

pub async fn execute(package_dir: PathBuf, url: &str, node: Option<&str>) -> anyhow::Result<()> {
    if !package_dir.join("pkg").exists() {
        return Err(anyhow::anyhow!(
            "Required `pkg/` dir not found within given input dir {:?} (or cwd, if none given). Please re-run targeting a package.",
            package_dir,
        ));
    }
    let pkg_dir = package_dir.join("pkg").canonicalize()?;
    let metadata: serde_json::Value = serde_json::from_reader(fs::File::open(
        pkg_dir.join("metadata.json")
    )?)?;
    let package_name = metadata["package"].as_str().unwrap();
    let publisher = metadata["publisher"].as_str().unwrap();
    let pkg_publisher = format!("{}:{}", package_name, publisher);
    println!("{}", pkg_publisher);

    // Create zip and put it in /target
    let parent_dir = pkg_dir.parent().unwrap();
    let target_dir = parent_dir.join("target");
    fs::create_dir_all(&target_dir)?;
    let zip_filename = target_dir.join(&pkg_publisher).with_extension("zip");
    zip_directory(&pkg_dir, &zip_filename.to_str().unwrap())?;

    // Create and send new package request
    let new_pkg_request = new_package(
        node,
        package_name,
        publisher,
        zip_filename.to_str().unwrap(),
    )?;
    let response = inject_message::send_request(url, new_pkg_request).await?;
    let inject_message::Response { ref ipc, .. } = inject_message::parse_response(response).await?;
    let ipc = serde_json::from_str::<serde_json::Value>(ipc)?;
    let new_package_response = ipc.get("NewPackageResponse");

    if new_package_response != Some(&serde_json::Value::String("Success".to_string())) {
        let error_message = format!("Failed to add package. Got response from node: {}", ipc);
        println!("{}", error_message);
        return Err(anyhow::anyhow!(error_message));
    }

    // Install package
    let install_request = interact_with_package("Install", node, package_name, publisher)?;
    let response = inject_message::send_request(url, install_request).await?;
    let inject_message::Response { ref ipc, .. } = inject_message::parse_response(response).await?;
    let ipc = serde_json::from_str::<serde_json::Value>(ipc)?;
    let install_response = ipc.get("InstallResponse");

    if install_response == Some(&serde_json::Value::String("Success".to_string())) {
        println!("Successfully installed package {} on node at {}", pkg_publisher, url);
    } else {
        let error_message = format!("Failed to start package. Got response from node: {}", ipc);
        println!("{}", error_message);
        return Err(anyhow::anyhow!(error_message));
    }

    Ok(())
}
