use std::fs::File;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{info, instrument};

use super::setup::{
    check_js_deps, check_py_deps, check_rust_deps, get_deps, get_newest_valid_node_version,
    REQUIRED_PY_PACKAGE,
};

const PY_VENV_NAME: &str = "process_env";
const JAVASCRIPT_SRC_PATH: &str = "src/lib.js";
const PYTHON_SRC_PATH: &str = "src/lib.py";
const RUST_SRC_PATH: &str = "src/lib.rs";
const KINODE_WIT_URL: &str =
    "https://raw.githubusercontent.com/kinode-dao/kinode-wit/v0.8.0-alpha/kinode.wit";
const WASI_VERSION: &str = "17.0.1"; // TODO: un-hardcode
pub const CACHE_DIR: &str = "/tmp/kinode-kit-cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoFile {
    package: CargoPackage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoPackage {
    name: String,
}

#[instrument(level = "trace", err, skip_all)]
pub fn run_command(cmd: &mut Command) -> anyhow::Result<()> {
    let output = cmd.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Command `{} {:?}` failed with exit code {}\nstdout: {}\nstderr: {}",
            cmd.get_program().to_str().unwrap(),
            cmd.get_args()
                .map(|a| a.to_str().unwrap())
                .collect::<Vec<_>>(),
            output.status.code().unwrap(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ))
    }
}

#[instrument(level = "trace", err, skip_all)]
pub async fn download_file(url: &str, path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(&CACHE_DIR).await?;
    let hex_url = hex::encode(url);
    let hex_url_path = format!("{}/{}", CACHE_DIR, hex_url);
    let hex_url_path = Path::new(&hex_url_path);

    let content = if hex_url_path.exists() {
        fs::read(hex_url_path).await?
    } else {
        let response = reqwest::get(url).await?;

        // Check if response status is 200 (OK)
        if response.status() != reqwest::StatusCode::OK {
            return Err(anyhow::anyhow!(
                "Failed to download file: HTTP Status {}",
                response.status()
            ));
        }

        let content = response.bytes().await?.to_vec();
        fs::write(hex_url_path, &content).await?;
        content
    };

    if path.exists() {
        if path.is_dir() {
            fs::remove_dir_all(path).await?;
        } else {
            let existing_content = fs::read(path).await?;
            if content == existing_content {
                return Ok(());
            }
        }
    }
    fs::create_dir_all(
        path.parent()
            .ok_or(anyhow::anyhow!("path doesn't have parent"))?,
    )
    .await?;
    fs::write(path, &content).await?;
    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
async fn compile_javascript_wasm_process(
    process_dir: &Path,
    valid_node: Option<String>,
) -> anyhow::Result<()> {
    info!(
        "Compiling Javascript Kinode process in {:?}...",
        process_dir
    );
    let wit_dir = process_dir.join("wit");
    download_file(KINODE_WIT_URL, &wit_dir.join("kinode.wit")).await?;

    let wasm_file_name = process_dir.file_name().and_then(|s| s.to_str()).unwrap();

    let install = "npm install".to_string();
    let componentize = format!("node componentize.mjs {wasm_file_name}");
    let (install, componentize) = valid_node
        .map(|valid_node| {
            (
                format!(
                    "source ~/.nvm/nvm.sh && nvm use {} && {}",
                    valid_node, install
                ),
                format!(
                    "source ~/.nvm/nvm.sh && nvm use {} && {}",
                    valid_node, componentize
                ),
            )
        })
        .unwrap_or_else(|| (install, componentize));

    run_command(
        Command::new("bash")
            .args(&["-c", &install])
            .current_dir(process_dir),
    )?;

    run_command(
        Command::new("bash")
            .args(&["-c", &componentize])
            .current_dir(process_dir),
    )?;

    info!(
        "Done compiling Javascript Kinode process in {:?}.",
        process_dir
    );
    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
async fn compile_python_wasm_process(process_dir: &Path, python: &str) -> anyhow::Result<()> {
    info!("Compiling Python Kinode process in {:?}...", process_dir);
    let wit_dir = process_dir.join("wit");
    download_file(KINODE_WIT_URL, &wit_dir.join("kinode.wit")).await?;

    let wasm_file_name = process_dir.file_name().and_then(|s| s.to_str()).unwrap();

    run_command(
        Command::new(python)
            .args(&["-m", "venv", PY_VENV_NAME])
            .current_dir(process_dir),
    )?;
    run_command(Command::new("bash")
        .args(&[
            "-c",
            &format!("source ../{PY_VENV_NAME}/bin/activate && pip install {REQUIRED_PY_PACKAGE} && componentize-py -d ../wit/ -w process componentize lib -o ../../pkg/{wasm_file_name}.wasm"),
        ])
        .current_dir(process_dir.join("src"))
    )?;

    info!("Done compiling Python Kinode process in {:?}.", process_dir);
    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
async fn compile_rust_wasm_process(process_dir: &Path) -> anyhow::Result<()> {
    info!("Compiling Rust Kinode process in {:?}...", process_dir);

    // Paths
    let bindings_dir = process_dir
        .join("target")
        .join("bindings")
        .join(process_dir.file_name().unwrap());
    let wit_dir = process_dir.join("wit");

    // Ensure the bindings directory exists
    fs::create_dir_all(&bindings_dir).await?;

    // Check and download kinode.wit if wit_dir does not exist
    download_file(KINODE_WIT_URL, &wit_dir.join("kinode.wit")).await?;

    // Check and download wasi_snapshot_preview1.wasm if it does not exist
    let wasi_snapshot_file = process_dir.join("wasi_snapshot_preview1.wasm");
    let wasi_snapshot_url = format!(
        "https://github.com/bytecodealliance/wasmtime/releases/download/v{}/wasi_snapshot_preview1.reactor.wasm",
        WASI_VERSION,
    );
    download_file(&wasi_snapshot_url, &wasi_snapshot_file).await?;

    // Create target.wasm (compiled .wit) & world
    run_command(Command::new("wasm-tools").args(&[
        "component",
        "wit",
        wit_dir.to_str().unwrap(),
        "-o",
        &bindings_dir.join("target.wasm").to_str().unwrap(),
        "--wasm",
    ]))?;

    // Copy wit directory to bindings
    fs::create_dir_all(&bindings_dir.join("wit")).await?;
    let mut entries = fs::read_dir(&wit_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        fs::copy(
            entry.path(),
            bindings_dir.join("wit").join(entry.file_name()),
        )
        .await?;
    }

    // Create an empty world file
    File::create(bindings_dir.join("world"))?;

    // Build the module using Cargo
    run_command(
        Command::new("cargo")
            .args(&[
                "+nightly",
                "build",
                "--release",
                "--no-default-features",
                "--target",
                "wasm32-wasi",
                "--target-dir",
                "target",
            ])
            .current_dir(process_dir),
    )?;

    // Adapt the module using wasm-tools

    // For use inside of process_dir
    let wasm_file_name = process_dir.file_name().and_then(|s| s.to_str()).unwrap();

    let wasm_file_prefix = Path::new("target/wasm32-wasi/release");
    let wasm_file = wasm_file_prefix.join(&format!("{}.wasm", wasm_file_name));
    let adapted_wasm_file = wasm_file_prefix.join(&format!("{}_adapted.wasm", wasm_file_name));

    let wasi_snapshot_file = Path::new("wasi_snapshot_preview1.wasm");

    run_command(
        Command::new("wasm-tools")
            .args(&[
                "component",
                "new",
                wasm_file.to_str().unwrap(),
                "-o",
                adapted_wasm_file.to_str().unwrap(),
                "--adapt",
                wasi_snapshot_file.to_str().unwrap(),
            ])
            .current_dir(process_dir),
    )?;

    let wasm_path = format!("../pkg/{}.wasm", wasm_file_name);
    let wasm_path = Path::new(&wasm_path);

    // Embed wit into the component and place it in the expected location
    run_command(
        Command::new("wasm-tools")
            .args(&[
                "component",
                "embed",
                wit_dir.strip_prefix(process_dir).unwrap().to_str().unwrap(),
                "--world",
                "process",
                adapted_wasm_file.to_str().unwrap(),
                "-o",
                wasm_path.to_str().unwrap(),
            ])
            .current_dir(process_dir),
    )?;

    info!("Done compiling Rust Kinode process in {:?}.", process_dir);
    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
async fn compile_and_copy_ui(package_dir: &Path, valid_node: Option<String>) -> anyhow::Result<()> {
    let ui_path = package_dir.join("ui");
    info!("Building UI in {:?}...", ui_path);

    if ui_path.exists() && ui_path.is_dir() {
        if ui_path.join("package.json").exists() {
            info!("UI directory found, running npm install...");

            let install = "npm install".to_string();
            let run = "npm run build:copy".to_string();
            let (install, run) = valid_node
                .map(|valid_node| {
                    (
                        format!(
                            "source ~/.nvm/nvm.sh && nvm use {} && {}",
                            valid_node, install
                        ),
                        format!("source ~/.nvm/nvm.sh && nvm use {} && {}", valid_node, run),
                    )
                })
                .unwrap_or_else(|| (install, run));

            run_command(
                Command::new("bash")
                    .args(&["-c", &install])
                    .current_dir(&ui_path),
            )?;

            info!("Running npm run build:copy...");

            run_command(
                Command::new("bash")
                    .args(&["-c", &run])
                    .current_dir(&ui_path),
            )?;
        } else {
            let pkg_ui_path = package_dir.join("pkg/ui");
            if pkg_ui_path.exists() {
                fs::remove_dir_all(&pkg_ui_path).await?;
            }
            run_command(
                Command::new("cp")
                    .args(["-r", "ui", "pkg/ui"])
                    .current_dir(&package_dir),
            )?;
        }
    } else {
        return Err(anyhow::anyhow!("'ui' directory not found"));
    }

    info!("Done building UI in {:?}.", ui_path);
    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
async fn compile_package_and_ui(
    package_dir: &Path,
    valid_node: Option<String>,
    skip_deps_check: bool,
) -> anyhow::Result<()> {
    compile_and_copy_ui(package_dir, valid_node).await?;
    compile_package(package_dir, skip_deps_check).await?;
    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
async fn compile_package(package_dir: &Path, skip_deps_check: bool) -> anyhow::Result<()> {
    for entry in package_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if path.join(RUST_SRC_PATH).exists() {
                if !skip_deps_check {
                    let deps = check_rust_deps()?;
                    get_deps(deps)?;
                }
                compile_rust_wasm_process(&path).await?;
            } else if path.join(PYTHON_SRC_PATH).exists() {
                let python = check_py_deps()?;
                compile_python_wasm_process(&path, &python).await?;
            } else if path.join(JAVASCRIPT_SRC_PATH).exists() {
                if !skip_deps_check {
                    let deps = check_js_deps()?;
                    get_deps(deps)?;
                }
                let valid_node = get_newest_valid_node_version(None, None)?;
                compile_javascript_wasm_process(&path, valid_node).await?;
            }
        }
    }

    Ok(())
}

#[instrument(level = "trace", err, skip_all)]
pub async fn execute(
    package_dir: &Path,
    no_ui: bool,
    ui_only: bool,
    skip_deps_check: bool,
) -> anyhow::Result<()> {
    if !package_dir.join("pkg").exists() {
        return Err(anyhow::anyhow!(
            "Required `pkg/` dir not found within given input dir {:?} (or cwd, if none given). Please re-run targeting a package.",
            package_dir,
        ));
    }

    let ui_dir = package_dir.join("ui");
    if !ui_dir.exists() {
        if ui_only {
            return Err(anyhow::anyhow!(
                "kit build: can't build UI: no ui directory exists"
            ));
        } else {
            compile_package(package_dir, skip_deps_check).await
        }
    } else {
        if no_ui {
            return compile_package(package_dir, skip_deps_check).await;
        }

        let deps = check_js_deps()?;
        get_deps(deps)?;
        let valid_node = get_newest_valid_node_version(None, None)?;

        if ui_only {
            compile_and_copy_ui(package_dir, valid_node).await
        } else {
            compile_package_and_ui(package_dir, valid_node, skip_deps_check).await
        }
    }
}
