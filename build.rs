use std::fs;
use std::io::{self, Write};
use std::path::Path;

const TEMPLATES_DIR: &str = "src/new/templates";
const TARGET_DIR: &str = "target";
const INCLUDES: &str = "includes.rs";

fn visit_dirs(dir: &Path, output_buffer: &mut Vec<u8>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().and_then(|s| s.to_str());
            if dir_name == Some("home") || dir_name == Some("target") {
                continue;
            }
            visit_dirs(&path, output_buffer)?;
        } else {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("swp") || ext == Some("wasm") || ext == Some("zip") {
                continue;
            }
            let file_name = path.file_name().and_then(|s| s.to_str());
            if file_name == Some("Cargo.lock") {
                continue;
            }

            let relative_path = path.strip_prefix(TEMPLATES_DIR).unwrap();
            let path_str = relative_path.to_str().unwrap().replace("\\", "/");

            let relative_path_from_includes = Path::new("..").join(path);
            let path_str_from_includes = relative_path_from_includes
                .to_str()
                .unwrap()
                .replace("\\", "/");
            writeln!(
                output_buffer,
                "    (\"{}\", include_str!(\"{}\")),",
                path_str, path_str_from_includes,
            )?;
        }
    }
    Ok(())
}

fn add_commit_hash(repo: &git2::Repository) -> anyhow::Result<()> {
    let sha = repo
        .head()?
        .target()
        .ok_or(anyhow::anyhow!("couldn't get commit hash"))?;

    println!("cargo:rustc-env=GIT_COMMIT_SHA={}", sha);

    Ok(())
}

fn add_branch_name(repo: &git2::Repository) -> anyhow::Result<()> {
    let head = repo.head()?;
    let branch = head
        .shorthand()
        .ok_or(anyhow::anyhow!("couldn't get branch name"))?;

    println!("cargo:rustc-env=GIT_BRANCH_NAME={}", branch);

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut output_buffer = Vec::new();
    writeln!(
        &mut output_buffer,
        "const PATH_TO_CONTENT: &[(&str, &str)] = &["
    )?;
    writeln!(
        output_buffer,
        "    (\"{}\", include_str!(\"{}\")),",
        "componentize.mjs", "../src/new/componentize.mjs",
    )?;

    visit_dirs(Path::new(TEMPLATES_DIR), &mut output_buffer)?;

    writeln!(&mut output_buffer, "];")?;

    let target_dir = Path::new(TARGET_DIR);
    let output_path = target_dir.join(INCLUDES);
    // create includes.rs if it does not exist
    if !target_dir.exists() {
        fs::create_dir_all(target_dir)?;
    }
    if !output_path.exists() {
        fs::write(&output_path, &output_buffer)?;
    } else {
        let existing_file = fs::read(&output_path)?;
        if output_buffer != existing_file {
            fs::write(&output_path, &output_buffer)?;
        }
    }

    let repo = git2::Repository::open(".")?;

    add_commit_hash(&repo)?;
    add_branch_name(&repo)?;

    Ok(())
}
