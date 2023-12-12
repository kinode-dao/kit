use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use reqwest;

pub fn run_command(cmd: &mut Command) -> io::Result<()> {
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "Command failed"))
    }
}

pub async fn download_file(url: &str, path: &Path) -> anyhow::Result<()> {
    let response = reqwest::get(url).await?;

    // Check if response status is 200 (OK)
    if response.status() != reqwest::StatusCode::OK {
        return Err(anyhow::anyhow!("Failed to download file: HTTP Status {}", response.status()));
    }

    let content = response.bytes().await?;

    fs::write(path, &content)?;
    Ok(())
}

pub async fn compile_package(package_dir: &Path, verbose: bool) -> anyhow::Result<()> {
    // Check if `Cargo.toml` exists in the directory
    let cargo_file = package_dir.join("Cargo.toml");
    if cargo_file.exists() {
        compile_wasm_project(package_dir, false, verbose).await?;
    } else {
        // If `Cargo.toml` is not found, look for subdirectories containing `Cargo.toml`
        for entry in package_dir.read_dir()? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.join("Cargo.toml").exists() {
                compile_wasm_project(&path, true, verbose).await?;
            }
        }
    }

    Ok(())
}

pub async fn compile_wasm_project(process_dir: &Path, is_subdir: bool, verbose: bool) -> anyhow::Result<()> {
    println!("Compiling WASM project in {:?}...", process_dir);

    // Paths
    let bindings_dir = process_dir
        .join("target")
        .join("bindings")
        .join(process_dir.file_name().unwrap());
    let wit_dir = process_dir.join("wit");

    // Ensure the bindings directory exists
    fs::create_dir_all(&bindings_dir)?;

    // Check and download uqbar.wit if wit_dir does not exist
    if !wit_dir.exists() {
        fs::create_dir_all(&wit_dir)?;
        let uqbar_wit_url = "https://raw.githubusercontent.com/uqbar-dao/uqwit/master/uqbar.wit";
        download_file(uqbar_wit_url, &wit_dir.join("uqbar.wit")).await?;
    }

    // Check and download wasi_snapshot_preview1.wasm if it does not exist
    let wasi_snapshot_file = process_dir.join("wasi_snapshot_preview1.wasm");
    if !wasi_snapshot_file.exists() {
        let wasi_version = "15.0.1";  // TODO: un-hardcode
        let wasi_snapshot_url = format!(
            "https://github.com/bytecodealliance/wasmtime/releases/download/v{}/wasi_snapshot_preview1.reactor.wasm",
            wasi_version,
        );
        download_file(&wasi_snapshot_url, &wasi_snapshot_file).await?;
    }

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
        .current_dir(process_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    // Adapt the module using wasm-tools

    // For use inside of process_dir
    let wasm_file_prefix = Path::new("target/wasm32-wasi/release");
    let wasm_file = wasm_file_prefix
        .clone()
        .join(&format!("{}.wasm", process_dir.file_name().unwrap().to_str().unwrap()));
    let adapted_wasm_file = wasm_file_prefix
        .clone()
        .join(&format!("{}_adapted.wasm", process_dir.file_name().unwrap().to_str().unwrap()));

    let wasi_snapshot_file = Path::new("wasi_snapshot_preview1.wasm");

    run_command(Command::new("wasm-tools")
        .args(&["component", "new",
            wasm_file.to_str().unwrap(),
            "-o", adapted_wasm_file.to_str().unwrap(),
            "--adapt", wasi_snapshot_file.to_str().unwrap(),
        ])
        .current_dir(process_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    let wasm_path =
        if is_subdir {
            format!("../pkg/{}.wasm", process_dir.file_name().unwrap().to_str().unwrap())
        } else {
            format!("pkg/{}.wasm", process_dir.file_name().unwrap().to_str().unwrap())
        };
    let wasm_path = Path::new(&wasm_path);

    // Embed wit into the component and place it in the expected location
    run_command(Command::new("wasm-tools")
        .args(&["component", "embed",
            wit_dir.strip_prefix(process_dir).unwrap().to_str().unwrap(),
            "--world", "process",
            adapted_wasm_file.to_str().unwrap(),
            "-o", wasm_path.to_str().unwrap(),
        ])
        .current_dir(process_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    println!("Done compiling WASM project in {:?}.", process_dir);
    Ok(())
}
