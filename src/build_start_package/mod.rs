use std::collections::HashSet;
use std::path::{Path, PathBuf};

use color_eyre::Result;
use tracing::instrument;

use crate::build;
use crate::start_package;

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    package_dir: &Path,
    no_ui: bool,
    ui_only: bool,
    include: &HashSet<PathBuf>,
    exclude: &HashSet<PathBuf>,
    url: &str,
    skip_deps_check: bool,
    features: &str,
    download_from: Option<&str>,
    default_world: Option<&str>,
    local_dependencies: Vec<PathBuf>,
    add_paths_to_api: Vec<PathBuf>,
    no_rewrite: bool,
    reproducible: bool,
    force: bool,
    verbose: bool,
) -> Result<()> {
    build::execute(
        package_dir,
        no_ui,
        ui_only,
        include,
        exclude,
        skip_deps_check,
        features,
        Some(url.into()),
        download_from,
        default_world,
        local_dependencies,
        add_paths_to_api,
        no_rewrite,
        reproducible,
        force,
        verbose,
        false,
    )
    .await?;
    start_package::execute(package_dir, url).await?;
    Ok(())
}
