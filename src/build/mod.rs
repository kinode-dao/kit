use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use reqwest;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoFile {
    package: CargoPackage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoPackage {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Metadata {
    package: String,
    publisher: String,
    version: [u32; 3],
}

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

async fn compile_python_wasm_process(
    process_dir: &Path,
    verbose: bool,
) -> anyhow::Result<()> {
    println!("Compiling Python Uqbar process in {:?}...", process_dir);
    let wit_dir = process_dir.join("wit");
    fs::create_dir_all(&wit_dir)?;
    let uqbar_wit_url = "https://raw.githubusercontent.com/uqbar-dao/uqwit/master/uqbar.wit";
    download_file(uqbar_wit_url, &wit_dir.join("uqbar.wit")).await?;

    let wasm_file_name = process_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap();

    run_command(Command::new("componentize-py")
        .args(&[
            "-d", "../wit/",
            "-w", "process",
            "componentize", "lib",
            "-o", &format!("../../pkg/{wasm_file_name}.wasm")
        ])
        .current_dir(process_dir.join("src"))
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    println!("Done compiling Python Uqbar process in {:?}.", process_dir);
    Ok(())
}

async fn compile_rust_wasm_process(
    process_dir: &Path,
    verbose: bool,
) -> anyhow::Result<()> {
    println!("Compiling Rust Uqbar process in {:?}...", process_dir);

    // Paths
    let bindings_dir = process_dir
        .join("target")
        .join("bindings")
        .join(process_dir.file_name().unwrap());
    let wit_dir = process_dir.join("wit");

    // Ensure the bindings directory exists
    fs::create_dir_all(&bindings_dir)?;

    // Check and download uqbar.wit if wit_dir does not exist
    //if !wit_dir.exists() { // TODO: do a smarter check; this check will fail when remote has updated v
    fs::create_dir_all(&wit_dir)?;
    let uqbar_wit_url = "https://raw.githubusercontent.com/uqbar-dao/uqwit/master/uqbar.wit";
    download_file(uqbar_wit_url, &wit_dir.join("uqbar.wit")).await?;

    // Check and download wasi_snapshot_preview1.wasm if it does not exist
    let wasi_snapshot_file = process_dir.join("wasi_snapshot_preview1.wasm");
    //if !wasi_snapshot_file.exists() { // TODO: do a smarter check; this check will fail when remote has updated v
    let wasi_version = "15.0.1";  // TODO: un-hardcode
    let wasi_snapshot_url = format!(
        "https://github.com/bytecodealliance/wasmtime/releases/download/v{}/wasi_snapshot_preview1.reactor.wasm",
        wasi_version,
    );
    download_file(&wasi_snapshot_url, &wasi_snapshot_file).await?;

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
    let wasm_file_name = {
        let cargo_path = process_dir.join("Cargo.toml");
        let cargo_contents = fs::read_to_string(cargo_path)?;
        let cargo_parsed = toml::from_str::<CargoFile>(&cargo_contents)?;
        cargo_parsed.package.name
    };

    let wasm_file_prefix = Path::new("target/wasm32-wasi/release");
    let wasm_file = wasm_file_prefix
        .clone()
        .join(&format!("{}.wasm", wasm_file_name));
        // .join(&format!("{}.wasm", process_dir.file_name().unwrap().to_str().unwrap()));
    let adapted_wasm_file = wasm_file_prefix
        .clone()
        .join(&format!("{}_adapted.wasm", wasm_file_name));
        // .join(&format!("{}_adapted.wasm", process_dir.file_name().unwrap().to_str().unwrap()));

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

    let wasm_path = format!("../pkg/{}.wasm", wasm_file_name);
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

    println!("Done compiling Rust Uqbar process in {:?}.", process_dir);
    Ok(())
}

pub fn develop_ui(package_dir: &Path, url: &str) -> anyhow::Result<()> {
    let ui_path = package_dir.join("ui");
    println!("Starting development UI in {:?}...", ui_path);

    if ui_path.exists() && ui_path.is_dir() && ui_path.join("package.json").exists() {
        println!("UI directory found, running npm install...");

        run_command(Command::new("npm")
            .arg("install")
            .current_dir(&ui_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
        )?;

        println!("Running npm start...");

        run_command(Command::new("npm")
            .arg("start")
            .env("VITE_NODE_URL", url)
            .current_dir(&ui_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
        )?;
    } else {
        println!("'ui' directory not found or 'ui/package.json' does not exist");
    }

    Ok(())
}

fn compile_and_copy_ui(package_dir: &Path, verbose: bool) -> anyhow::Result<()> {
    let ui_path = package_dir.join("ui");
    println!("Building UI in {:?}...", ui_path);

    if ui_path.exists() && ui_path.is_dir() && ui_path.join("package.json").exists() {
        println!("UI directory found, running npm install...");

        // Set the current directory to 'ui_path' for the command
        run_command(Command::new("npm")
            .arg("install")
            .current_dir(&ui_path) // Set the working directory
            .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
            .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
        )?;

        println!("Running npm run build:copy...");

        // Similarly for 'npm run build:copy'
        run_command(Command::new("npm")
            .args(["run", "build:copy"])
            .current_dir(&ui_path) // Set the working directory
            .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
            .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
        )?;
    } else {
        println!("'ui' directory not found or 'ui/package.json' does not exist");
    }

    Ok(())
}

async fn compile_package_and_ui(package_dir: &Path, verbose: bool) -> anyhow::Result<()> {
    compile_package(package_dir, verbose).await?;
    compile_and_copy_ui(package_dir, verbose)?;
    Ok(())
}

async fn compile_package(package_dir: &Path, verbose: bool) -> anyhow::Result<()> {
    let rust_src_path = "src/lib.rs";
    let python_src_path = "src/lib.py";
    for entry in package_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if path.join(&rust_src_path).exists() {
                compile_rust_wasm_process(&path, verbose).await?;
            } else if path.join(&python_src_path).exists() {
                compile_python_wasm_process(&path, verbose).await?;
            }
        }
    }

    Ok(())
}

pub async fn execute(package_dir: &Path, ui_only: bool, verbose: bool) -> anyhow::Result<()> {
    let ui_dir = package_dir.join("ui");
    if !ui_dir.exists() {
        if ui_only {
            return Err(anyhow::anyhow!("uqdev build: can't build UI: no ui directory exists"));
        } else {
            compile_package(package_dir, verbose).await
        }
    } else {
        if ui_only {
            compile_and_copy_ui(package_dir, verbose)
        } else {
            compile_package_and_ui(package_dir, verbose).await
        }
    }
}
