use std::path::PathBuf;

use fs_err as fs;
use tracing::{info, instrument};

use crate::KIT_CACHE;

#[instrument(level = "trace", err(Debug), skip_all)]
fn reset_cache() -> anyhow::Result<()> {
    info!("Resetting cache...");
    let path = PathBuf::from(KIT_CACHE);
    if path.exists() {
        fs::remove_dir_all(&path)?;
    }
    info!("Done resetting cache.");
    Ok(())
}

#[instrument(level = "trace", err(Debug), skip_all)]
pub fn execute() -> anyhow::Result<()> {
    reset_cache()
}
