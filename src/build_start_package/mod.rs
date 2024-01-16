use std::path::Path;

use super::build;
use super::start_package;

pub async fn execute(
    package_dir: &Path,
    ui_only: bool,
    verbose: bool,
    url: &str,
) -> anyhow::Result<()> {
    build::execute(package_dir, ui_only, verbose).await?;
    start_package::execute(package_dir, url).await?;
    Ok(())
}
