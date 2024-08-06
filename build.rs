use std::fs;
use std::io::{self, Write};
use std::path::Path;

const NEW_DIR: &str = "src/new";
const TEMPLATES_DIR: &str = "src/new/templates";

fn visit_dirs(dir: &Path, output_buffer: &mut Vec<u8>) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, output_buffer)?;
            } else {
                if path.extension().and_then(|s| s.to_str()) == Some("swp") {
                    continue;
                }

                let relative_path = path.strip_prefix(TEMPLATES_DIR).unwrap();
                let path_str = relative_path.to_str().unwrap().replace("\\", "/");

                let relative_path_from_includes = path.strip_prefix(NEW_DIR).unwrap();
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
        "componentize.mjs", "componentize.mjs",
    )?;

    visit_dirs(Path::new(TEMPLATES_DIR), &mut output_buffer)?;

    writeln!(&mut output_buffer, "];")?;

    let output_path = Path::new(NEW_DIR).join("includes.rs");
    // create includes.rs if it does not exist
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
