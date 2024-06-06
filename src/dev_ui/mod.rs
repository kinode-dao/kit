use std::path::Path;
use std::process::Command;

use color_eyre::{eyre::eyre, Result};
use tracing::{info, instrument};

use crate::build::run_command;
use crate::setup::{check_js_deps, get_deps, get_newest_valid_node_version};

#[instrument(level = "trace", skip_all)]
pub fn execute(
    package_dir: &Path,
    url: &str,
    skip_deps_check: bool,
    release: bool,
) -> Result<()> {
    if !skip_deps_check {
        let deps = check_js_deps()?;
        get_deps(deps, false)?;
    }
    let valid_node = get_newest_valid_node_version(None, None)?;

    let ui_path = package_dir.join("ui");
    info!("Starting development UI in {:?}...", ui_path);

    if ui_path.exists() && ui_path.is_dir() && ui_path.join("package.json").exists() {
        info!("UI directory found, running npm install...");

        let install = "npm install".to_string();
        let dev = if release {
            "npm start".to_string()
        } else {
            "npm run dev".to_string()
        };
        let (install_command, dev_command) = valid_node
            .map(|valid_node| {(
                format!("source ~/.nvm/nvm.sh && nvm use {} && {}", valid_node, install),
                format!("source ~/.nvm/nvm.sh && nvm use {} && {}", valid_node, dev),
            )})
            .unwrap_or_else(|| (install, dev.clone()));

        run_command(
            Command::new("bash")
                .args(&["-c", &install_command])
                .current_dir(&ui_path),
            false,
        )?;

        info!("Running {}", dev);

        run_command(
            Command::new("bash")
                .args(&["-c", &dev_command])
                .env("VITE_NODE_URL", url)
                .current_dir(&ui_path),
            false,
        )?;
    } else {
        return Err(eyre!("'ui' directory not found or 'ui/package.json' does not exist"));
    }

    Ok(())
}
