use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use color_eyre::{eyre::{eyre, WrapErr}, Result};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, instrument};

use kinode_process_lib::kernel_types::Erc721Metadata;

use crate::KIT_CACHE;
use crate::setup::{
    check_js_deps, check_py_deps, check_rust_deps, get_deps, get_newest_valid_node_version, get_python_version,
    REQUIRED_PY_PACKAGE,
};
use crate::start_package::zip_directory;
use crate::view_api;

const PY_VENV_NAME: &str = "process_env";
const JAVASCRIPT_SRC_PATH: &str = "src/lib.js";
const PYTHON_SRC_PATH: &str = "src/lib.py";
const RUST_SRC_PATH: &str = "src/lib.rs";
const KINODE_WIT_URL: &str =
    "https://raw.githubusercontent.com/kinode-dao/kinode-wit/aa2c8b11c9171b949d1991c32f58591c0e881f85/kinode.wit";
const WASI_VERSION: &str = "19.0.1"; // TODO: un-hardcode
const DEFAULT_WORLD: &str = "process";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoFile {
    package: CargoPackage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoPackage {
    name: String,
}

#[instrument(level = "trace", skip_all)]
pub fn run_command(cmd: &mut Command) -> Result<(String, String)> {
    let output = cmd.output()?;
    if output.status.success() {
        Ok((
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    } else {
        Err(eyre!(
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

#[instrument(level = "trace", skip_all)]
pub async fn download_file(url: &str, path: &Path) -> Result<()> {
    fs::create_dir_all(&KIT_CACHE)?;
    let hex_url = hex::encode(url);
    let hex_url_path = format!("{}/{}", KIT_CACHE, hex_url);
    let hex_url_path = Path::new(&hex_url_path);

    let content = if hex_url_path.exists() {
        fs::read(hex_url_path)?
    } else {
        let response = reqwest::get(url).await?;

        // Check if response status is 200 (OK)
        if response.status() != reqwest::StatusCode::OK {
            return Err(eyre!("Failed to download file: HTTP Status {}", response.status()));
        }

        let content = response.bytes().await?.to_vec();
        fs::write(hex_url_path, &content)?;
        content
    };

    if path.exists() {
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            let existing_content = fs::read(path)?;
            if content == existing_content {
                return Ok(());
            }
        }
    }
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| eyre!("path doesn't have parent"))?,
    )?;
    fs::write(path, &content)?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub fn read_metadata(package_dir: &Path) -> Result<Erc721Metadata> {
    let metadata: Erc721Metadata =
        serde_json::from_reader(fs::File::open(package_dir.join("metadata.json"))
            .wrap_err_with(|| "Missing required metadata.json file. See discussion at https://book.kinode.org/my_first_app/chapter_1.html?highlight=metadata.json#metadatajson")?
        )?;
    Ok(metadata)
}

/// Regex to dynamically capture the world name after 'world'
#[instrument(level = "trace", skip_all)]
fn extract_world(data: &str) -> Option<String> {
    let re = regex::Regex::new(r"world\s+([^\s\{]+)").unwrap();
    re.captures(data).and_then(|caps| caps.get(1).map(|match_| match_.as_str().to_string()))
}

#[instrument(level = "trace", skip_all)]
fn extract_worlds_from_files(directory: &Path) -> Vec<String> {
    let mut worlds = vec![];

    // Safe to return early if directory reading fails
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return worlds,
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file()
        || Some("kinode.wit") == path.file_name().and_then(|s| s.to_str())
        || Some("wit") != path.extension().and_then(|s| s.to_str()) {
            continue;
        }
        let contents = fs::read_to_string(&path).unwrap_or_default();
        if let Some(world) = extract_world(&contents) {
            worlds.push(world);
        }
    }

    worlds
}

#[instrument(level = "trace", skip_all)]
fn get_world_or_default(directory: &Path, default_world: Option<String>) -> String {
    let worlds = extract_worlds_from_files(directory);
    if worlds.len() == 1 {
        return worlds[0].clone();
    }
    let default_world = default_world.unwrap_or_else(|| DEFAULT_WORLD.to_string());
    warn!("Found {} worlds in {directory:?}; defaulting to {default_world}", worlds.len());
    default_world
}

#[instrument(level = "trace", skip_all)]
async fn compile_javascript_wasm_process(
    process_dir: &Path,
    valid_node: Option<String>,
    default_world: Option<String>,
) -> Result<()> {
    info!(
        "Compiling Javascript Kinode process in {:?}...",
        process_dir
    );

    let wasm_file_name = process_dir.file_name().and_then(|s| s.to_str()).unwrap();
    let world_name = get_world_or_default(&process_dir.join("target").join("wit"), default_world);

    let install = "npm install".to_string();
    let componentize = format!("node componentize.mjs {wasm_file_name} {world_name}");
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

#[instrument(level = "trace", skip_all)]
async fn compile_python_wasm_process(
    process_dir: &Path,
    python: &str,
    default_world: Option<String>,
) -> Result<()> {
    info!("Compiling Python Kinode process in {:?}...", process_dir);

    let wasm_file_name = process_dir.file_name().and_then(|s| s.to_str()).unwrap();
    let world_name = get_world_or_default(&process_dir.join("target").join("wit"), default_world);

    let source = format!("source ../{PY_VENV_NAME}/bin/activate");
    let install = format!("pip install {REQUIRED_PY_PACKAGE}");
    let componentize = format!(
        "componentize-py -d ../target/wit/ -w {} componentize lib -o ../../pkg/{}.wasm",
        world_name,
        wasm_file_name,
    );

    run_command(
        Command::new(python)
            .args(&["-m", "venv", PY_VENV_NAME])
            .current_dir(process_dir),
    )?;
    run_command(Command::new("bash")
        .args(&["-c", &format!("{source} && {install} && {componentize}")])
        .current_dir(process_dir.join("src"))
    )?;

    info!("Done compiling Python Kinode process in {:?}.", process_dir);
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn compile_rust_wasm_process(
    process_dir: &Path,
    features: &str,
) -> Result<()> {
    info!("Compiling Rust Kinode process in {:?}...", process_dir);

    // Paths
    let wit_dir = process_dir.join("target").join("wit");
    let bindings_dir = process_dir
        .join("target")
        .join("bindings")
        .join(process_dir.file_name().unwrap());
    fs::create_dir_all(&bindings_dir)?;

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
    fs::create_dir_all(&bindings_dir.join("wit"))?;
    for entry in fs::read_dir(&wit_dir)? {
        let entry = entry?;
        fs::copy(
            entry.path(),
            bindings_dir.join("wit").join(entry.file_name()),
        )?;
    }

    // Create an empty world file
    fs::File::create(bindings_dir.join("world"))?;

    // Build the module using Cargo
    let mut args = vec![
         "+nightly",
         "build",
         "--release",
         "--no-default-features",
         "--target",
         "wasm32-wasi",
         "--target-dir",
         "target",
         "--color=always"
    ];
    if !features.is_empty() {
        args.push("--features");
        args.push(&features);
    }
    let (stdout, stderr) = run_command(
        Command::new("cargo")
            .args(&args)
            .current_dir(process_dir),
    )?;
    if stdout.contains("warning") {
        warn!("{}", stdout);
    }
    if stderr.contains("warning") {
        warn!("{}", stderr);
    }

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

#[instrument(level = "trace", skip_all)]
async fn compile_and_copy_ui(package_dir: &Path, valid_node: Option<String>) -> Result<()> {
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
                fs::remove_dir_all(&pkg_ui_path)?;
            }
            run_command(
                Command::new("cp")
                    .args(["-r", "ui", "pkg/ui"])
                    .current_dir(&package_dir),
            )?;
        }
    } else {
        return Err(eyre!("'ui' directory not found"));
    }

    info!("Done building UI in {:?}.", ui_path);
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn compile_package_and_ui(
    package_dir: &Path,
    valid_node: Option<String>,
    skip_deps_check: bool,
    features: &str,
    url: Option<String>,
    default_world: Option<String>,
) -> Result<()> {
    compile_and_copy_ui(package_dir, valid_node).await?;
    compile_package(package_dir, skip_deps_check, features, url, default_world).await?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn build_wit_dir(process_dir: &Path, apis: &HashMap<String, Vec<u8>>) -> Result<()> {
    let wit_dir = process_dir.join("target").join("wit");
    download_file(KINODE_WIT_URL, &wit_dir.join("kinode.wit")).await?;
    for (file_name, contents) in apis {
        fs::write(wit_dir.join(file_name), contents)?;
    }

    //let src_dir = process_dir.join("src");
    //for entry in src_dir.read_dir()? {
    //    let entry = entry?;
    //    let path = entry.path();
    //    if Some("wit") == path.extension().and_then(|s| s.to_str()) {
    //        if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
    //            fs::copy(&path, wit_dir.join(file_name))?;
    //        }
    //    }
    //}
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn compile_package_item(
    entry: std::io::Result<std::fs::DirEntry>,
    features: String,
    apis: HashMap<String, Vec<u8>>,
    default_world: Option<String>,
) -> Result<()> {
    let entry = entry?;
    let path = entry.path();
    if path.is_dir() {
        let is_rust_process = path.join(RUST_SRC_PATH).exists();
        let is_py_process = path.join(PYTHON_SRC_PATH).exists();
        let is_js_process = path.join(JAVASCRIPT_SRC_PATH).exists();
        if is_rust_process || is_py_process || is_js_process {
            build_wit_dir(&path, &apis).await?;
        }

        if is_rust_process {
            compile_rust_wasm_process(&path, &features).await?;
        } else if is_py_process {
            let python = get_python_version(None, None)?
                .ok_or_else(|| eyre!("kit requires Python 3.10 or newer"))?;
            compile_python_wasm_process(&path, &python, default_world).await?;
        } else if is_js_process {
            let valid_node = get_newest_valid_node_version(None, None)?;
            compile_javascript_wasm_process(&path, valid_node, default_world).await?;
        }
    }
    Ok(())
}

async fn fetch_dependencies(
    dependencies: &Vec<String>,
    apis: &mut HashMap<String, Vec<u8>>,
    url: String,
) -> Result<()> {
    for dependency in dependencies {
        let Some(zip_dir) = view_api::execute(None, Some(dependency), &url, false).await? else {
            return Err(eyre!("Got unexpected result from fetching API for {dependency}"));
        };
        for entry in zip_dir.read_dir()? {
            let entry = entry?;
            let path = entry.path();
            if Some("wit") == path.extension().and_then(|s| s.to_str()) {
                let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
                let wit_contents = fs::read(&path)?;
                apis.insert(file_name.into(), wit_contents);
            }
        }
    }
    Ok(())
}

/// package dir looks like:
/// ```
/// metadata.json
/// api/                                  <- optional
///   my_package:publisher.os-v0.wit
/// pkg/
///   api.zip                             <- built
///   manifest.json
///   process_i.wasm                      <- built
///   projess_j.wasm                      <- built
/// process_i/
///   src/
///     lib.rs
///   target/                             <- built
///     api/
///     wit/
/// process_j/
///   src/
///   target/                             <- built
///     api/
///     wit/
/// ```

#[instrument(level = "trace", skip_all)]
async fn compile_package(
    package_dir: &Path,
    skip_deps_check: bool,
    features: &str,
    url: Option<String>,
    default_world: Option<String>,
) -> Result<()> {
    let metadata = read_metadata(package_dir)?;
    let mut checked_rust = false;
    let mut checked_py = false;
    let mut checked_js = false;
    let mut apis = HashMap::new();
    for entry in package_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if path.join(RUST_SRC_PATH).exists() && !checked_rust && !skip_deps_check {
                let deps = check_rust_deps()?;
                get_deps(deps)?;
                checked_rust = true;
            } else if path.join(PYTHON_SRC_PATH).exists() && !checked_py {
                check_py_deps()?;
                checked_py = true;
            } else if path.join(JAVASCRIPT_SRC_PATH).exists() && !checked_js && !skip_deps_check {
                let deps = check_js_deps()?;
                get_deps(deps)?;
                checked_js = true;
            } else if Some("api") == path.file_name().and_then(|s| s.to_str()) {
                // zip & place in pkg/: publish API inside Kinode
                let zip_path = package_dir.join("pkg").join("api.zip");
                let zip_path = zip_path.to_str().unwrap();
                zip_directory(&path, zip_path)?;

                // read api files: to be used in build
                for entry in path.read_dir()? {
                    let entry = entry?;
                    let path = entry.path();
                    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                        continue;
                    };
                    if ext != "wit" {
                        continue;
                    }
                    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if file_name.starts_with(&format!(
                        "{}:{}",
                        metadata.properties.package_name,
                        metadata.properties.publisher,
                    )) {
                        if let Ok(api_contents) = fs::read(&path) {
                            apis.insert(file_name.to_string(), api_contents);
                        }
                    }
                }

                // fetch dependency apis: to be used in build
                if let Some(ref dependencies) = metadata.properties.dependencies {
                    let Some(ref url) = url else {
                        // TODO: can we use kit-cached deps?
                        return Err(eyre!("Need a node to be able to fetch dependencies"));
                    };
                    fetch_dependencies(
                        dependencies,
                        &mut apis,
                        url.clone(),
                    ).await?;
                }
            }
        }
    }

    let mut tasks = tokio::task::JoinSet::new();
    let features = features.to_string();
    for entry in package_dir.read_dir()? {
        tasks.spawn(compile_package_item(
            entry,
            features.clone(),
            apis.clone(),
            default_world.clone(),
        ));
    }
    while let Some(res) = tasks.join_next().await {
        res??;
    }

    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    package_dir: &Path,
    no_ui: bool,
    ui_only: bool,
    skip_deps_check: bool,
    features: &str,
    url: Option<String>,
    default_world: Option<String>,
) -> Result<()> {
    if !package_dir.join("pkg").exists() {
        if Some(".DS_Store") == package_dir.file_name().and_then(|s| s.to_str()) {
            info!("Skipping build of {:?}", package_dir);
            return Ok(());
        }
        return Err(eyre!(
            "Required `pkg/` dir not found within given input dir {:?} (or cwd, if none given). Please re-run targeting a package.",
            package_dir,
        ));
    }

    let ui_dir = package_dir.join("ui");
    if !ui_dir.exists() {
        if ui_only {
            return Err(eyre!("kit build: can't build UI: no ui directory exists"));
        } else {
            compile_package(package_dir, skip_deps_check, features, url, default_world).await
        }
    } else {
        if no_ui {
            return compile_package(
                package_dir,
                skip_deps_check,
                features,
                url,
                default_world,
            ).await;
        }

        let deps = check_js_deps()?;
        get_deps(deps)?;
        let valid_node = get_newest_valid_node_version(None, None)?;

        if ui_only {
            compile_and_copy_ui(package_dir, valid_node).await
        } else {
            compile_package_and_ui(
                package_dir,
                valid_node,
                skip_deps_check,
                features,
                url,
                default_world,
            ).await
        }
    }
}
