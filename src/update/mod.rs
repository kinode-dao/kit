use std::process::Command;

use tracing::instrument;

use super::build;

#[instrument(level = "trace", err, skip_all)]
pub fn execute(mut user_args: Vec<String>, branch: &str) -> anyhow::Result<()> {
    let mut args: Vec<String> = vec!["install",
        "--git", "https://github.com/kinode-dao/kit",
        "--branch", branch,
    ]
        .iter()
        .map(|v| v.to_string())
        .collect();
    args.append(&mut user_args);
    build::run_command(Command::new("cargo").args(&args[..]))?;

    let cache_path = format!("{}/kinode-dao-kit-commits", build::CACHE_DIR);
    let cache_path = std::path::Path::new(&cache_path);
    if cache_path.exists() {
        std::fs::remove_dir_all(&cache_path)?;
    }
    Ok(())
}
