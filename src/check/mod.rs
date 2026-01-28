use crate::build::run_command;
use color_eyre::eyre::{eyre, Result};
use fs_err as fs;
use std::path::Path;
use std::process::Command;
use tracing::{info, instrument};

#[instrument(level = "trace", skip_all)]
pub fn execute(
    package_dir: &Path,
    release: bool,
    profile: Option<&str>,
    targets: Vec<String>,
    packages: Vec<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    verbose: bool,
) -> Result<()> {
    let package_dir = fs::canonicalize(package_dir)?;

    if !package_dir.exists() {
        let error = format!("Directory {:?} doesnt exists.", package_dir);
        return Err(eyre!(error));
    }

    info!("Checking package in {:?}...", package_dir);

    let mut args: Vec<String> = vec!["check", "--target-dir", "target"]
        .iter()
        .map(|v| v.to_string())
        .collect();

    if release {
        args.push("--release".to_string());
    }

    if let Some(prof) = profile {
        args.push("--profile".to_string());
        args.push(prof.to_string());
    }

    for target in targets {
        args.push("--target".to_string());
        args.push(target);
    }

    for package in packages {
        args.push("--package".to_string());
        args.push(package);
    }

    if all_features {
        args.push("--all-features".to_string());
    }

    if no_default_features {
        args.push("--no-default-features".to_string());
    }

    if !features.is_empty() {
        args.push("--features".to_string());
        args.push(features.join(","));
    }

    run_command(
        Command::new("cargo")
            .args(&args[..])
            .current_dir(&package_dir),
        verbose,
    )?;

    info!("Done checking package in {:?}.", package_dir);
    Ok(())
}
