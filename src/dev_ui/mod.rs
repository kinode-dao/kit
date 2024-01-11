use std::path::Path;
use std::process::{Command, Stdio};

use super::build::run_command;
use super::setup::{check_js_deps, get_deps};

pub fn execute(package_dir: &Path, url: &str) -> anyhow::Result<()> {
    let deps = check_js_deps()?;
    get_deps(deps)?;
    let ui_path = package_dir.join("ui");
    println!("Starting development UI in {:?}...", ui_path);

    if ui_path.exists() && ui_path.is_dir() && ui_path.join("package.json").exists() {
        println!("UI directory found, running npm install...");

        run_command(Command::new("npm")
            .arg("install")
            .current_dir(&ui_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
        )?;

        println!("Running npm start...");

        run_command(Command::new("npm")
            .arg("start")
            .env("VITE_NODE_URL", url)
            .current_dir(&ui_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
        )?;
    } else {
        println!("'ui' directory not found or 'ui/package.json' does not exist");
    }

    Ok(())
}
