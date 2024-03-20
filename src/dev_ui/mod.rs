use std::path::Path;
use std::process::Command;

use tracing::{info, instrument};

use super::build::run_command;
use super::setup::{check_js_deps, get_deps, get_newest_valid_node_version};

#[instrument(level = "trace", err, skip_all)]
pub fn execute(
    package_dir: &Path,
    url: &str,
    skip_deps_check: bool,
    release: bool,
) -> anyhow::Result<()> {
    if !skip_deps_check {
        let deps = check_js_deps()?;
        get_deps(deps)?;
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
        let (install, dev) = valid_node
            .map(|valid_node| {
                (
                    format!(
                        "source ~/.nvm/nvm.sh && nvm use {} && {}",
                        valid_node, install
                    ),
                    format!("source ~/.nvm/nvm.sh && nvm use {} && {}", valid_node, dev),
                )
            })
            .unwrap_or_else(|| (install, dev));

        run_command(
            Command::new("bash")
                .args(&["-c", &install])
                .current_dir(&ui_path),
        )?;

        if release {
            info!("Running npm start");
        } else {
            info!("Running npm run dev...");
        }

        run_command(
            Command::new("bash")
                .args(&["-c", &dev])
                .env("VITE_NODE_URL", url)
                .current_dir(&ui_path),
        )?;
    } else {
        return Err(anyhow::anyhow!(
            "'ui' directory not found or 'ui/package.json' does not exist"
        ));
    }

    Ok(())
}
