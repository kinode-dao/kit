use std::fs;
use std::path::PathBuf;
use std::process;

use super::inject_message;
use super::start_package::interact_with_package;

pub async fn execute(
    project_dir: PathBuf,
    url: &str,
    node: Option<&str>,
    arg_package_name: Option<&str>,
    arg_publisher: Option<&str>,
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

    println!("Successfully removed package {}:{} on node at {}", package_name, publisher, url);

    Ok(())
}
