use std::path::PathBuf;

use color_eyre::Result;
use fs_err as fs;
use tracing::{info, instrument};

use crate::KIT_CACHE;

#[instrument(level = "trace", skip_all)]
fn reset_cache() -> Result<()> {
    info!("Resetting cache...");
    let path = PathBuf::from(KIT_CACHE);
    if path.exists() {
        fs::remove_dir_all(&path)?;
    }
    info!("Done resetting cache.");
    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub fn execute() -> Result<()> {
    reset_cache()
}
