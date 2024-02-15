use std::path::Path;
use std::process::Command;

use super::build::run_command;
use super::setup::{check_js_deps, get_deps, get_newest_valid_node_version};

#[autocontext::autocontext]
pub fn execute(package_dir: &Path, url: &str, skip_deps_check: bool) -> anyhow::Result<()> {
    if !skip_deps_check {
        let deps = check_js_deps()?;
        get_deps(deps)?;
    }
    let valid_node = get_newest_valid_node_version(None, None)?;

    let ui_path = package_dir.join("ui");
    println!("Starting development UI in {:?}...", ui_path);

    if ui_path.exists() && ui_path.is_dir() && ui_path.join("package.json").exists() {
        println!("UI directory found, running npm install...");

        let install = "npm install".to_string();
        let start = "npm start".to_string();
        let (install, start) = valid_node
            .map(|valid_node| {(
                format!("source ~/.nvm/nvm.sh && nvm use {} && {}", valid_node, install),
                format!("source ~/.nvm/nvm.sh && nvm use {} && {}", valid_node, start),
            )})
            .unwrap_or_else(|| (install, start));

        run_command(Command::new("bash")
            .args(&["-c", &install])
            .current_dir(&ui_path)
        )?;

        println!("Running npm start...");

        run_command(Command::new("bash")
            .args(&["-c", &start])
            .env("VITE_NODE_URL", url)
            .current_dir(&ui_path)
        )?;
    } else {
        println!("'ui' directory not found or 'ui/package.json' does not exist");
    }

    Ok(())
}
