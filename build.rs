use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

const NEW_DIR: &str = "src/new";
const TEMPLATES_DIR: &str = "src/new/templates";

fn main() -> io::Result<()> {
    let output_path = Path::new(NEW_DIR).join("includes.rs");
    let mut output_file = File::create(output_path)?;

    writeln!(&mut output_file, "const PATH_TO_CONTENT: &[(&str, &str)] = &[")?;

    writeln!(
        output_file,
        "    (\"{}\", include_str!(\"{}\")),",
        "componentize.mjs",
        "componentize.mjs",
    )?;

    visit_dirs(Path::new(TEMPLATES_DIR), &mut output_file)?;

    writeln!(&mut output_file, "];")?;
    Ok(())
}

fn visit_dirs(dir: &Path, output_file: &mut File) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, output_file)?;
            } else {
                if path.extension().and_then(|s| s.to_str()) == Some("swp") {
                    continue;
                }

                let relative_path = path.strip_prefix(TEMPLATES_DIR).unwrap();
                let path_str = relative_path.to_str().unwrap().replace("\\", "/");

                let relative_path_from_includes = path.strip_prefix(NEW_DIR).unwrap();
                let path_str_from_includes = relative_path_from_includes.to_str().unwrap().replace("\\", "/");
                writeln!(
                    output_file,
                    "    (\"{}\", include_str!(\"{}\")),",
                    path_str,
                    path_str_from_includes,
                )?;
            }
        }
    }
    Ok(())
}

