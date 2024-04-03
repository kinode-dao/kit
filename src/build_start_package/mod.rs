use std::path::Path;

use color_eyre::Result;
use tracing::instrument;

use crate::build;
use crate::start_package;

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    package_dir: &Path,
    no_ui: bool,
    ui_only: bool,
    url: &str,
    skip_deps_check: bool,
    features: &str,
) -> Result<()> {
    build::execute(package_dir, no_ui, ui_only, skip_deps_check, features).await?;
    start_package::execute(package_dir, url).await?;
    Ok(())
}
