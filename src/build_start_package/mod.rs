use std::path::Path;

use tracing::instrument;

use super::build;
use super::start_package;

#[instrument(level = "trace", err, skip_all)]
pub async fn execute(
    package_dir: &Path,
    no_ui: bool,
    ui_only: bool,
    url: &str,
    skip_deps_check: bool,
    features: &str,
) -> anyhow::Result<()> {
    build::execute(package_dir, no_ui, ui_only, skip_deps_check, features).await?;
    start_package::execute(package_dir, url).await?;
    Ok(())
}
