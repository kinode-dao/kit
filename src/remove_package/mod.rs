use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use serde_json::json;
use walkdir::WalkDir;
use zip::write::FileOptions;

use super::inject_message;

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

pub async fn execute(
    project_dir: PathBuf,
    url: &str,
    node: Option<&str>,
    arg_package_name: Option<&str>,
    arg_publisher: Option<&str>,
    is_delete: bool,
) -> anyhow::Result<()> {
    let (package_name, publisher): (String, String) = match (arg_package_name, arg_publisher) {
        (Some(package_name), Some(publisher)) => (package_name.into(), publisher.into()),
        _ => {
            let pkg_dir = project_dir.join("pkg").canonicalize()?;
            let metadata: serde_json::Value = serde_json::from_reader(fs::File::open(pkg_dir
                .join("metadata.json")
            )?)?;
            let package_name = metadata["package"].as_str().unwrap();
            let publisher = metadata["publisher"].as_str().unwrap();
            (package_name.into(), publisher.into())
        },
    };

    // Create and send uninstall request
    let uninstall_request = interact_with_package(
        "Uninstall",
        node,
        &package_name,
        &publisher,
    )?;
    let response = inject_message::send_request(
        url,
        uninstall_request,
    ).await?;
    if response.status() != 200 {
        process::exit(1);
    }

    // Delete package, if desired
    if is_delete {
        let delete_request = interact_with_package(
            "Delete",
            node,
            &package_name,
            &publisher,
        )?;
        let response = inject_message::send_request(
            url,
            delete_request,
        ).await?;
        if response.status() != 200 {
            process::exit(1);
        }
    }

    println!("Successfully removed package: {}:{}", package_name, publisher);

    Ok(())
}
