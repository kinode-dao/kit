use std::path::{Path, PathBuf};

use color_eyre::Result;
use fs_err as fs;
use regex::Regex;
use tracing::{debug, instrument};

#[instrument(level = "trace", skip_all)]
pub fn copy_and_rewrite_package(package_dir: &Path) -> Result<PathBuf> {
    // Create target/rewrite/ directory
    let rewrite_dir = package_dir.join("target").join("rewrite");
    if rewrite_dir.exists() {
        fs::remove_dir_all(&rewrite_dir)?;
    }
    fs::create_dir_all(&rewrite_dir)?;

    // Copy package contents
    copy_dir_and_rewrite(package_dir, &rewrite_dir)?;

    Ok(rewrite_dir)
}

#[instrument(level = "trace", skip_all)]
fn copy_dir_and_rewrite(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            // Skip target/ directory to avoid recursion
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            copy_dir_and_rewrite(&path, &dest_path)?;
        } else {
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                // Rewrite Rust files
                let contents = fs::read_to_string(&path)?;
                let new_contents = rewrite_rust_file(&contents)?;
                debug!("rewrote {}", dest_path.display());
                fs::write(&dest_path, new_contents)?;
            } else {
                // Copy other files as-is
                fs::copy(&path, &dest_path)?;
            }
        }
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn rewrite_rust_file(content: &str) -> Result<String> {
    let println_re = Regex::new(r#"(\s*)println!\("(.*)"(.*)\)"#)?;
    let result = println_re.replace_all(content, r#"${1}println!("hi ${2}"${3})"#);
    Ok(result.into_owned())
}
