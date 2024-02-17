use std::path::Path;

use tracing::instrument;

use super::build;
use super::start_package;

#[instrument(level = "trace", err, skip_all)]
pub async fn execute(
    package_dir: &Path,
    no_ui: bool,
    ui_only: bool,
    verbose: bool,
    url: &str,
    skip_deps_check: bool,
) -> anyhow::Result<()> {
    build::execute(package_dir, no_ui, ui_only, verbose, skip_deps_check).await?;
    start_package::execute(package_dir, url).await?;
    Ok(())
}
