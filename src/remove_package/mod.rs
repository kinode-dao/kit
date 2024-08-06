use std::path::Path;
use std::process;

use color_eyre::{eyre::WrapErr, Result};
use fs_err as fs;
use tracing::{info, instrument};

use kinode_process_lib::kernel_types::Erc721Metadata;

use crate::inject_message;
use crate::start_package::interact_with_package;

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    project_dir: &Path,
    url: &str,
    arg_package_name: Option<&str>,
    arg_publisher: Option<&str>,
) -> Result<()> {
    let (package_name, publisher): (String, String) = match (arg_package_name, arg_publisher) {
        (Some(package_name), Some(publisher)) => (package_name.into(), publisher.into()),
        _ => {
            let metadata: Erc721Metadata = serde_json::from_reader(fs::File::open(
                    project_dir.join("metadata.json")
                )
                .wrap_err_with(|| "Missing required metadata.json file. See discussion at https://book.kinode.org/my_first_app/chapter_1.html?highlight=metadata.json#metadatajson")?
            )?;
            let package_name = metadata.properties.package_name.as_str();
            let publisher = metadata.properties.publisher.as_str();
            (package_name.into(), publisher.into())
        }
    };

    // Create and send uninstall request
    let uninstall_request = interact_with_package("Uninstall", None, &package_name, &publisher)?;
    let response = inject_message::send_request(url, uninstall_request).await?;
    if response.status() != 200 {
        process::exit(1);
    }

    info!(
        "Successfully removed package {}:{} on node at {}",
        package_name, publisher, url
    );

    Ok(())
}
