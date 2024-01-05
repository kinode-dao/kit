use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Stdio};

use reqwest;
use serde::{Serialize, Deserialize};

use super::setup::{check_js_deps, check_py_deps, check_rust_deps, get_deps, REQUIRED_PY_PACKAGE};

const PY_VENV_NAME: &str = "process_env";
const JAVASCRIPT_SRC_PATH: &str = "src/lib.js";
const PYTHON_SRC_PATH: &str = "src/lib.py";
const RUST_SRC_PATH: &str = "src/lib.rs";

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

pub fn run_command(cmd: &mut Command) -> anyhow::Result<()> {
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Command `{} {:?}` failed with exit code {}",
            cmd.get_program().to_str().unwrap(),
            cmd.get_args().map(|a| a.to_str().unwrap()).collect::<Vec<_>>(),
            status.code().unwrap(),
        ))
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

async fn compile_javascript_wasm_process(
    process_dir: &Path,
    verbose: bool,
) -> anyhow::Result<()> {
    println!("Compiling Javascript Uqbar process in {:?}...", process_dir);
    let wit_dir = process_dir.join("wit");
    fs::create_dir_all(&wit_dir)?;
    let uqbar_wit_url = "https://raw.githubusercontent.com/uqbar-dao/uqwit/master/uqbar.wit";
    download_file(uqbar_wit_url, &wit_dir.join("uqbar.wit")).await?;

    let wasm_file_name = process_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap();

    run_command(Command::new("npm")
        .arg("install")
        .current_dir(process_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    run_command(Command::new("node")
        .args(&["componentize.mjs", wasm_file_name])
        .current_dir(process_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;

    println!("Done compiling Javascript Uqbar process in {:?}.", process_dir);
    Ok(())
}

async fn compile_python_wasm_process(
    process_dir: &Path,
    python: &str,
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

    run_command(Command::new(python)
        .args(&["-m", "venv", PY_VENV_NAME])
        .current_dir(process_dir)
        .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
        .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
    )?;
    run_command(Command::new("bash")
        .args(&[
            "-c",
            &format!("source ../{PY_VENV_NAME}/bin/activate && pip install {REQUIRED_PY_PACKAGE} && componentize-py -d ../wit/ -w process componentize lib -o ../../pkg/{wasm_file_name}.wasm"),
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
    let wasm_file_name = process_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap();

    let wasm_file_prefix = Path::new("target/wasm32-wasi/release");
    let wasm_file = wasm_file_prefix
        .join(&format!("{}.wasm", wasm_file_name));
    let adapted_wasm_file = wasm_file_prefix
        .join(&format!("{}_adapted.wasm", wasm_file_name));

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

fn compile_and_copy_ui(package_dir: &Path, verbose: bool) -> anyhow::Result<()> {
    let ui_path = package_dir.join("ui");
    println!("Building UI in {:?}...", ui_path);

    if ui_path.exists() && ui_path.is_dir() {
        if ui_path.join("package.json").exists() {
            println!("UI directory found, running npm install...");

            run_command(Command::new("npm")
                .arg("install")
                .current_dir(&ui_path)
                .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
                .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
            )?;

            println!("Running npm run build:copy...");

            run_command(Command::new("npm")
                .args(["run", "build:copy"])
                .current_dir(&ui_path)
                .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
                .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
            )?;
        } else {
            let pkg_ui_path = package_dir.join("pkg/ui");
            if pkg_ui_path.exists() {
                fs::remove_dir_all(&pkg_ui_path)?;
            }
            run_command(Command::new("cp")
                .args(["-r", "ui", "pkg/ui"])
                .current_dir(&package_dir)
                .stdout(if verbose { Stdio::inherit() } else { Stdio::null() })
                .stderr(if verbose { Stdio::inherit() } else { Stdio::null() })
            )?;
        }
    } else {
        println!("'ui' directory not found");
    }

    println!("Done building UI in {:?}.", ui_path);
    Ok(())
}

async fn compile_package_and_ui(package_dir: &Path, verbose: bool) -> anyhow::Result<()> {
    compile_package(package_dir, verbose).await?;
    compile_and_copy_ui(package_dir, verbose)?;
    Ok(())
}

async fn compile_package(package_dir: &Path, verbose: bool) -> anyhow::Result<()> {
    for entry in package_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if path.join(RUST_SRC_PATH).exists() {
                let deps = check_rust_deps()?;
                get_deps(deps)?;
                compile_rust_wasm_process(&path, verbose).await?;
            } else if path.join(PYTHON_SRC_PATH).exists() {
                let python = check_py_deps()?;
                compile_python_wasm_process(&path, &python, verbose).await?;
            } else if path.join(JAVASCRIPT_SRC_PATH).exists() {
                let deps = check_js_deps()?;
                get_deps(deps)?;
                compile_javascript_wasm_process(&path, verbose).await?;
            }
        }
    }

    Ok(())
}

pub async fn execute(package_dir: &Path, ui_only: bool, verbose: bool) -> anyhow::Result<()> {
    if !package_dir.join("pkg").exists() {
        return Err(anyhow::anyhow!(
            "Required `pkg/` dir not found within given input dir {:?} (or cwd, if none given). Please re-run targeting a package.",
            package_dir,
        ));
    }
    let ui_dir = package_dir.join("ui");
    if !ui_dir.exists() {
        if ui_only {
            return Err(anyhow::anyhow!("uqdev build: can't build UI: no ui directory exists"));
        } else {
            compile_package(package_dir, verbose).await
        }
    } else {
        let deps = check_js_deps()?;
        get_deps(deps)?;
        if ui_only {
            compile_and_copy_ui(package_dir, verbose)
        } else {
            compile_package_and_ui(package_dir, verbose).await
        }
    }
}
