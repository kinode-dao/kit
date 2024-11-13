use std::path::Path;

use color_eyre::{eyre::eyre, Result};
use tracing::{info, instrument};

use crate::build::read_and_update_metadata;
use crate::inject_message;

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    package_dir: &Path,
    url: &str,
    arg_package_name: Option<&str>,
    arg_publisher: Option<&str>,
) -> Result<()> {
    let (package_name, publisher): (String, String) = match (arg_package_name, arg_publisher) {
        (Some(package_name), Some(publisher)) => (package_name.into(), publisher.into()),
        _ => {
            let metadata = read_and_update_metadata(package_dir)?;
            let package_name = metadata.properties.package_name.as_str();
            let publisher = metadata.properties.publisher.as_str();
            (package_name.into(), publisher.into())
        }
    };

    // Create and send uninstall request
    let body = serde_json::json!({
        "Uninstall": {"package_name": package_name, "publisher_node": publisher},
    });
    let uninstall_request = inject_message::make_message(
        "main:app-store:sys",
        Some(15),
        &body.to_string(),
        None,
        None,
        None,
    )?;
    let response = inject_message::send_request(url, uninstall_request).await?;
    let inject_message::Response { ref body, .. } =
        inject_message::parse_response(response).await?;
    let body = serde_json::from_str::<serde_json::Value>(body)?;

    let uninstall_response = body.get("UninstallResponse");

    if uninstall_response == Some(&serde_json::Value::String("Success".to_string())) {
        info!(
            "Successfully removed package {}:{} on node at {}",
            package_name, publisher, url
        );
    } else {
        return Err(eyre!(
            "Failed to remove package. Got response from node: {}",
            body
        ));
    }

    Ok(())
}
