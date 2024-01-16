use std::path::Path;

use super::build;
use super::start_package;

pub async fn execute(
    package_dir: &Path,
    ui_only: bool,
    verbose: bool,
    url: &str,
    skip_deps_check: bool,
) -> anyhow::Result<()> {
    build::execute(package_dir, ui_only, verbose, skip_deps_check).await?;
    start_package::execute(package_dir, url).await?;
    Ok(())
}
