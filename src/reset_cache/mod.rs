use std::fs::remove_dir_all;
use std::path::PathBuf;

use tracing::{info, instrument};

use crate::KIT_CACHE;

#[instrument(level = "trace", err, skip_all)]
fn reset_cache() -> anyhow::Result<()> {
    info!("Resetting cache...");
    let path = PathBuf::from(KIT_CACHE);
    if path.exists() {
        remove_dir_all(&path)?;
    }
    info!("Done resetting cache.");
    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
pub fn execute() -> anyhow::Result<()> {
    reset_cache()
}
