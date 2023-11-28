use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn run_command(cmd: &mut Command) -> io::Result<()> {
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "Command failed"))
    }
}

pub fn compile_process(process_dir: &Path, verbose: bool) -> io::Result<()> {
    // Check if `Cargo.toml` exists in the directory
    let cargo_file = process_dir.join("Cargo.toml");
    if cargo_file.exists() {
        compile_wasm_project(process_dir, verbose)?;
    } else {
        // If `Cargo.toml` is not found, look for subdirectories containing `Cargo.toml`
        for entry in process_dir.read_dir()? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.join("Cargo.toml").exists() {
                compile_wasm_project(&path, verbose)?;
            }
        }
    }

    Ok(())
}

pub fn compile_wasm_project(project_dir: &Path, verbose: bool) -> io::Result<()> {
    println!("Compiling WASM project in {:?}...", project_dir);

    // Paths
    let bindings_dir = project_dir
        .join("target")
        .join("bindings")
        .join(project_dir.file_name().unwrap());
    let wit_dir = project_dir.join("wit");

    // Ensure the bindings directory exists
    fs::create_dir_all(&bindings_dir)?;

    // Create target.wasm (compiled .wit) & world
    run_command(Command::new("wasm-tools")
        .args(&["component", "wit",
            wit_dir.to_str().unwrap(),
            "-o", &bindings_dir.join("target.wasm").to_str().unwrap(),
            "--wasm",
        ])
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    // Copy wit directory to bindings
    fs::create_dir_all(&bindings_dir.join("wit"))?;
    for entry in fs::read_dir(&wit_dir)? {
        let entry = entry?;
        fs::copy(entry.path(), bindings_dir.join("wit").join(entry.file_name()))?;
    }

    // Create an empty world file
    File::create(bindings_dir.join("world"))?;

    // Build the module using Cargo
    run_command(Command::new("cargo")
        .args(&["+nightly", "build",
            "--release",
            "--no-default-features",
            "--target", "wasm32-wasi",
        ])
        .current_dir(project_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    // Adapt the module using wasm-tools

    // For use inside of project_dir
    let wasm_file_prefix = Path::new("target/wasm32-wasi/release");
    let wasm_file = wasm_file_prefix
        .clone()
        .join(&format!("{}.wasm", project_dir.file_name().unwrap().to_str().unwrap()));
    let adapted_wasm_file = wasm_file_prefix
        .clone()
        .join(&format!("{}_adapted.wasm", project_dir.file_name().unwrap().to_str().unwrap()));

    let wasm_path = format!("pkg/{}.wasm", project_dir.file_name().unwrap().to_str().unwrap());
    let wasm_path = Path::new(&wasm_path);

    let wasi_snapshot_file = Path::new("wasi_snapshot_preview1.wasm");

    run_command(Command::new("wasm-tools")
        .args(&["component", "new",
            wasm_file.to_str().unwrap(),
            "-o", adapted_wasm_file.to_str().unwrap(),
            "--adapt", wasi_snapshot_file.to_str().unwrap(),
        ])
        .current_dir(project_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    // Embed "wit" into the component and place it in the expected location
    run_command(Command::new("wasm-tools")
        .args(&["component", "embed",
            wit_dir.strip_prefix(project_dir).unwrap().to_str().unwrap(),
            "--world", "process",
            adapted_wasm_file.to_str().unwrap(),
            "-o", wasm_path.to_str().unwrap(),
        ])
        .current_dir(project_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    println!("Done compiling WASM project in {:?}.", project_dir);
    Ok(())
}
