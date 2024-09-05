use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use color_eyre::{
    Section,
    {
        eyre::{eyre, WrapErr},
        Result,
    },
};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{debug, info, instrument, warn};
use walkdir::WalkDir;
use zip::write::FileOptions;

use kinode_process_lib::{PackageId, kernel_types::Erc721Metadata};

use crate::setup::{
    check_js_deps, check_py_deps, check_rust_deps, get_deps, get_newest_valid_node_version,
    get_python_version, REQUIRED_PY_PACKAGE,
};
use crate::view_api;
use crate::KIT_CACHE;

const PY_VENV_NAME: &str = "process_env";
const JAVASCRIPT_SRC_PATH: &str = "src/lib.js";
const PYTHON_SRC_PATH: &str = "src/lib.py";
const RUST_SRC_PATH: &str = "src/lib.rs";
const KINODE_WIT_0_7_0_URL: &str =
    "https://raw.githubusercontent.com/kinode-dao/kinode-wit/aa2c8b11c9171b949d1991c32f58591c0e881f85/kinode.wit";
const KINODE_WIT_0_8_0_URL: &str =
    "https://raw.githubusercontent.com/kinode-dao/kinode-wit/v0.8/kinode.wit";
const WASI_VERSION: &str = "19.0.1"; // TODO: un-hardcode
const DEFAULT_WORLD_0_7_0: &str = "process";
const DEFAULT_WORLD_0_8_0: &str = "process-v0";
const KINODE_PROCESS_LIB_CRATE_NAME: &str = "kinode_process_lib";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoFile {
    package: CargoPackage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoPackage {
    name: String,
}

pub fn make_pkg_publisher(metadata: &Erc721Metadata) -> String {
    let package_name = metadata.properties.package_name.as_str();
    let publisher = metadata.properties.publisher.as_str();
    let pkg_publisher = format!("{}:{}", package_name, publisher);
    pkg_publisher
}

pub fn make_zip_filename(package_dir: &Path, pkg_publisher: &str) -> PathBuf {
    let zip_filename =  package_dir.join("target").join(pkg_publisher).with_extension("zip");
    zip_filename
}

#[instrument(level = "trace", skip_all)]
pub fn hash_zip_pkg(zip_path: &Path) -> Result<String> {
    let mut file = fs::File::open(&zip_path)?;
    let mut hasher = Sha256::new();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    hasher.update(&buffer);
    let hash_result = hasher.finalize();
    Ok(format!("{hash_result:x}"))
}

#[instrument(level = "trace", skip_all)]
pub fn zip_pkg(package_dir: &Path, pkg_publisher: &str) -> Result<(PathBuf, String)> {
    let pkg_dir = package_dir.join("pkg");
    let target_dir = package_dir.join("target");
    fs::create_dir_all(&target_dir)?;
    let zip_filename = make_zip_filename(package_dir, pkg_publisher);
    zip_directory(&pkg_dir, &zip_filename.to_str().unwrap())?;

    let hash = hash_zip_pkg(&zip_filename)?;
    Ok((zip_filename, hash))
}

#[instrument(level = "trace", skip_all)]
fn zip_directory(directory: &Path, zip_filename: &str) -> Result<()> {
    let file = fs::File::create(zip_filename)?;
    let walkdir = WalkDir::new(directory);
    let it = walkdir.into_iter();

    let mut zip = zip::ZipWriter::new(file);

    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755)
        .last_modified_time(zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap());

    for entry in it {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(Path::new(directory))?;

        if path.is_file() {
            zip.start_file(name.to_string_lossy(), options)?;
            let mut f = fs::File::open(path)?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;
            zip.write_all(&*buffer)?;
        } else if name.as_os_str().len() != 0 {
            // Only if it is not the root directory
            zip.add_directory(name.to_string_lossy(), options)?;
        }
    }

    zip.finish()?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub fn has_feature(cargo_toml_path: &str, feature: &str) -> Result<bool> {
    let cargo_toml_content = fs::read_to_string(cargo_toml_path)?;
    let cargo_toml: toml::Value = cargo_toml_content.parse()?;

    if let Some(features) = cargo_toml.get("features").and_then(|f| f.as_table()) {
        Ok(features.contains_key(feature))
    } else {
        Ok(false)
    }
}

#[instrument(level = "trace", skip_all)]
pub fn remove_missing_features(
    cargo_toml_path: &Path,
    features: Vec<&str>,
) -> Result<Vec<String>> {
    let cargo_toml_content = fs::read_to_string(cargo_toml_path)?;
    let cargo_toml: toml::Value = cargo_toml_content.parse()?;
    let Some(cargo_features) = cargo_toml.get("features").and_then(|f| f.as_table()) else {
        return Ok(vec![]);
    };

    Ok(features
        .iter()
        .filter_map(|f| {
            let f = f.to_string();
            if cargo_features.contains_key(&f) {
                Some(f)
            } else {
                None
            }
        })
        .collect()
    )
}

/// Check if the first element is empty and there are no more elements
#[instrument(level = "trace", skip_all)]
fn is_only_empty_string(splitted: &Vec<&str>) -> bool {
    let mut parts = splitted.iter();
    parts.next() == Some(&"") && parts.next().is_none()
}

#[instrument(level = "trace", skip_all)]
pub fn run_command(cmd: &mut Command, verbose: bool) -> Result<Option<(String, String)>> {
    if verbose {
        let mut child = cmd.spawn()?;
        let result = child.wait()?;
        if result.success() {
            return Ok(None);
        } else {
            return Err(eyre!(
                "Command `{} {:?}` failed with exit code {:?}",
                cmd.get_program().to_str().unwrap(),
                cmd.get_args()
                    .map(|a| a.to_str().unwrap())
                    .collect::<Vec<_>>(),
                result.code(),
            ));
        }
    }
    let output = cmd.output()?;
    if output.status.success() {
        Ok(Some((
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )))
    } else {
        Err(eyre!(
            "Command `{} {:?}` failed with exit code {:?}\nstdout: {}\nstderr: {}",
            cmd.get_program().to_str().unwrap(),
            cmd.get_args()
                .map(|a| a.to_str().unwrap())
                .collect::<Vec<_>>(),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ))
    }
}

#[instrument(level = "trace", skip_all)]
pub async fn download_file(url: &str, path: &Path) -> Result<()> {
    fs::create_dir_all(&KIT_CACHE)?;
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let hashed_url = hasher.finalize();
    let hashed_url_path = Path::new(KIT_CACHE)
        .join(format!("{hashed_url:x}"));

    let content = if hashed_url_path.exists() {
        fs::read(hashed_url_path)?
    } else {
        let response = reqwest::get(url).await?;

        // Check if response status is 200 (OK)
        if response.status() != reqwest::StatusCode::OK {
            return Err(eyre!(
                "Failed to download file: HTTP Status {}",
                response.status()
            ));
        }

        let content = response.bytes().await?.to_vec();
        fs::write(hashed_url_path, &content)?;
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
fn extract_world(data: &str) -> Option<String> {
    let re = regex::Regex::new(r"world\s+([^\s\{]+)").unwrap();
    re.captures(data)
        .and_then(|caps| caps.get(1).map(|match_| match_.as_str().to_string()))
}

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
            || Some("wit") != path.extension().and_then(|s| s.to_str())
        {
            continue;
        }
        let contents = fs::read_to_string(&path).unwrap_or_default();
        if let Some(world) = extract_world(&contents) {
            worlds.push(world);
        }
    }

    worlds
}

fn get_world_or_default(directory: &Path, default_world: &str) -> String {
    let worlds = extract_worlds_from_files(directory);
    if worlds.len() == 1 {
        return worlds[0].clone();
    }
    warn!(
        "Found {} worlds in {directory:?}; defaulting to {default_world}",
        worlds.len()
    );
    default_world.to_string()
}

#[instrument(level = "trace", skip_all)]
fn copy_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn file_with_extension_exists(dir: &Path, extension: &str) -> bool {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
                return true;
            }
        }
    }
    false
}

#[instrument(level = "trace", skip_all)]
fn parse_version_from_url(url: &str) -> Result<semver::VersionReq> {
    let re = regex::Regex::new(r"\?tag=v([0-9]+\.[0-9]+\.[0-9]+)$").unwrap();
    if let Some(caps) = re.captures(url) {
        if let Some(version) = caps.get(1) {
            return Ok(semver::VersionReq::parse(&format!("^{}", version.as_str()))?);
        }
    }
    Err(eyre!("No valid version found in the URL"))
}

#[instrument(level = "trace", skip_all)]
fn find_crate_versions(
    crate_name: &str,
    packages: &HashMap<cargo_metadata::PackageId, &cargo_metadata::Package>,
) -> Result<HashMap<semver::VersionReq, Vec<String>>> {
    let mut versions = HashMap::new();

    // Iterate over all packages
    for package in packages.values() {
        // Check each dependency of the package
        for dependency in &package.dependencies {
            if dependency.name == crate_name {
                let version = if dependency.req != semver::VersionReq::default() {
                    dependency.req.clone()
                } else {
                    if let Some(ref source) = dependency.source {
                        parse_version_from_url(source)?
                    } else {
                        semver::VersionReq::default()
                    }
                };
                versions
                    .entry(version)
                    .or_insert_with(Vec::new)
                    .push(package.name.clone());
            }
        }
    }

    Ok(versions)
}

#[instrument(level = "trace", skip_all)]
fn check_process_lib_version(cargo_toml_path: &Path) -> Result<()> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(cargo_toml_path)
        .exec()?;
    let packages: HashMap<cargo_metadata::PackageId, &cargo_metadata::Package> = metadata
        .packages
        .iter()
        .map(|package| (package.id.clone(), package))
        .collect();
    let versions = find_crate_versions(KINODE_PROCESS_LIB_CRATE_NAME, &packages)?;
    if versions.len() > 1 {
        return Err(
            eyre!(
                "Found different versions of {} in different crates:{}",
                KINODE_PROCESS_LIB_CRATE_NAME,
                versions.iter().fold(String::new(), |s, (version, crates)| {
                    format!("{s}\n{version}\t{crates:?}")
                })
            )
            .with_suggestion(|| format!(
                "Set all {} versions to be the same to avoid hard-to-debug errors.",
                KINODE_PROCESS_LIB_CRATE_NAME,
            ))
        );
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn get_most_recent_modified_time(
    dir: &Path,
    exclude_files: &HashSet<&str>,
    exclude_extensions: &HashSet<&str>,
    exclude_dirs: &HashSet<&str>,
) -> Result<(Option<SystemTime>, Option<SystemTime>)> {
    let mut most_recent: Option<SystemTime> = None;
    let mut most_recent_excluded: Option<SystemTime> = None;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        let file_name = path.file_name().unwrap_or_default().to_str().unwrap_or_default();

        if exclude_files.contains(file_name) {
            let file_time = get_file_modified_time(&path)?;
            most_recent_excluded = Some(most_recent_excluded.map_or(file_time, |t| t.max(file_time)));
            continue;
        }

        if path.is_dir() {
            let dir_name = path.file_name().unwrap_or_default().to_str().unwrap_or_default();
            if exclude_dirs.contains(dir_name) {
                continue;
            }

            let (sub_time, sub_time_excluded) = get_most_recent_modified_time(
                &path,
                exclude_files,
                exclude_extensions,
                exclude_dirs,
            )?;

            if let Some(st) = sub_time {
                most_recent = Some(most_recent.map_or(st, |t| t.max(st)));
            }
            if let Some(ste) = sub_time_excluded {
                most_recent_excluded = Some(most_recent_excluded.map_or(ste, |t| t.max(ste)));
            }
        } else {
            if let Some(extension) = path.extension() {
                if exclude_extensions.contains(&extension.to_str().unwrap_or_default()) {
                    let file_time = get_file_modified_time(&path)?;
                    most_recent_excluded = Some(most_recent_excluded.map_or(file_time, |t| t.max(file_time)));
                    continue;
                }
            }

            let file_time = get_file_modified_time(&path)?;
            most_recent = Some(most_recent.map_or(file_time, |t| t.max(file_time)));
        }
    }

    Ok((most_recent, most_recent_excluded))
}

#[instrument(level = "trace", skip_all)]
fn get_file_modified_time(file_path: &Path) -> Result<SystemTime> {
    let metadata = fs::metadata(file_path)?;
    Ok(metadata.modified()?)
}

#[instrument(level = "trace", skip_all)]
async fn compile_javascript_wasm_process(
    process_dir: &Path,
    valid_node: Option<String>,
    world: &str,
    verbose: bool,
) -> Result<()> {
    info!(
        "Compiling Javascript Kinode process in {:?}...",
        process_dir
    );

    let wasm_file_name = process_dir.file_name().and_then(|s| s.to_str()).unwrap();
    let world_name = get_world_or_default(&process_dir.join("target").join("wit"), world);

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
        verbose,
    )?;

    run_command(
        Command::new("bash")
            .args(&["-c", &componentize])
            .current_dir(process_dir),
        verbose,
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
    world: &str,
    verbose: bool,
) -> Result<()> {
    info!("Compiling Python Kinode process in {:?}...", process_dir);

    let wasm_file_name = process_dir.file_name().and_then(|s| s.to_str()).unwrap();
    let world_name = get_world_or_default(&process_dir.join("target").join("wit"), world);

    let source = format!("source ../{PY_VENV_NAME}/bin/activate");
    let install = format!("pip install {REQUIRED_PY_PACKAGE}");
    let componentize = format!(
        "componentize-py -d ../target/wit/ -w {} componentize lib -o ../../pkg/{}.wasm",
        world_name, wasm_file_name,
    );

    run_command(
        Command::new(python)
            .args(&["-m", "venv", PY_VENV_NAME])
            .current_dir(process_dir),
        verbose,
    )?;
    run_command(
        Command::new("bash")
            .args(&["-c", &format!("{source} && {install} && {componentize}")])
            .current_dir(process_dir.join("src")),
        verbose,
    )?;

    info!("Done compiling Python Kinode process in {:?}.", process_dir);
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn compile_rust_wasm_process(
    process_dir: &Path,
    features: &str,
    verbose: bool,
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
    let wasi_snapshot_file = process_dir
        .join("target")
        .join("wasi_snapshot_preview1.wasm");
    let wasi_snapshot_url = format!(
        "https://github.com/bytecodealliance/wasmtime/releases/download/v{}/wasi_snapshot_preview1.reactor.wasm",
        WASI_VERSION,
    );
    download_file(&wasi_snapshot_url, &wasi_snapshot_file).await?;

    // Copy wit directory to bindings
    fs::create_dir_all(&bindings_dir.join("wit"))?;
    for entry in fs::read_dir(&wit_dir)? {
        let entry = entry?;
        fs::copy(
            entry.path(),
            bindings_dir.join("wit").join(entry.file_name()),
        )?;
    }

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
        "--color=always",
    ];
    let test_only = features == "test";
    let features: Vec<&str> = features.split(',').collect();
    let original_length = if is_only_empty_string(&features) {
        0
    } else {
        features.len()
    };
    let features = remove_missing_features(
        &process_dir.join("Cargo.toml"),
        features,
    )?;
    if !test_only && original_length != features.len() {
        info!("process {:?} missing features; using {:?}", process_dir, features);
    };
    let features = features.join(",");
    if !features.is_empty() {
        args.push("--features");
        args.push(&features);
    }
    let result = run_command(
        Command::new("cargo").args(&args).current_dir(process_dir),
        verbose,
    )?;

    if let Some((stdout, stderr)) = result {
        if stdout.contains("warning") {
            warn!("{}", stdout);
        }
        if stderr.contains("warning") {
            warn!("{}", stderr);
        }
    }

    // Adapt the module using wasm-tools

    // For use inside of process_dir
    let wasm_file_name = process_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap()
        .replace("-", "_");

    let wasm_file_prefix = Path::new("target/wasm32-wasi/release");
    let wasm_file = wasm_file_prefix.join(&format!("{}.wasm", wasm_file_name));

    let wasm_path = format!("../pkg/{}.wasm", wasm_file_name);
    let wasm_path = Path::new(&wasm_path);

    let wasi_snapshot_file = Path::new("target/wasi_snapshot_preview1.wasm");

    run_command(
        Command::new("wasm-tools")
            .args(&[
                "component",
                "new",
                wasm_file.to_str().unwrap(),
                "-o",
                wasm_path.to_str().unwrap(),
                "--adapt",
                wasi_snapshot_file.to_str().unwrap(),
            ])
            .current_dir(process_dir),
        verbose,
    )?;

    info!("Done compiling Rust Kinode process in {:?}.", process_dir);
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn compile_and_copy_ui(
    package_dir: &Path,
    valid_node: Option<String>,
    verbose: bool,
) -> Result<()> {
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
                verbose,
            )?;

            info!("Running npm run build:copy...");

            run_command(
                Command::new("bash")
                    .args(&["-c", &run])
                    .current_dir(&ui_path),
                verbose,
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
                verbose,
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
    default_world: Option<&str>,
    download_from: Option<&str>,
    local_dependencies: Vec<PathBuf>,
    add_paths_to_api: Vec<PathBuf>,
    force: bool,
    verbose: bool,
    ignore_deps: bool,
) -> Result<()> {
    compile_and_copy_ui(package_dir, valid_node, verbose).await?;
    compile_package(
        package_dir,
        skip_deps_check,
        features,
        url,
        default_world,
        download_from,
        local_dependencies,
        add_paths_to_api,
        force,
        verbose,
        ignore_deps,
    )
    .await?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn build_wit_dir(
    process_dir: &Path,
    apis: &HashMap<String, Vec<u8>>,
    wit_version: Option<u32>,
) -> Result<()> {
    let wit_dir = process_dir.join("target").join("wit");
    if wit_dir.exists() {
        fs::remove_dir_all(&wit_dir)?;
    }
    let wit_url = match wit_version {
        None => KINODE_WIT_0_7_0_URL,
        Some(0) | _ => KINODE_WIT_0_8_0_URL,
    };
    download_file(wit_url, &wit_dir.join("kinode.wit")).await?;
    for (file_name, contents) in apis {
        let destination = wit_dir.join(file_name);
        fs::write(&destination, contents)?;
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn compile_package_item(
    entry: std::io::Result<std::fs::DirEntry>,
    features: String,
    apis: HashMap<String, Vec<u8>>,
    world: String,
    wit_version: Option<u32>,
    verbose: bool,
) -> Result<()> {
    let entry = entry?;
    let path = entry.path();
    if path.is_dir() {
        let is_rust_process = path.join(RUST_SRC_PATH).exists();
        let is_py_process = path.join(PYTHON_SRC_PATH).exists();
        let is_js_process = path.join(JAVASCRIPT_SRC_PATH).exists();
        if is_rust_process || is_py_process || is_js_process {
            build_wit_dir(&path, &apis, wit_version).await?;
        }

        if is_rust_process {
            compile_rust_wasm_process(&path, &features, verbose).await?;
        } else if is_py_process {
            let python = get_python_version(None, None)?
                .ok_or_else(|| eyre!("kit requires Python 3.10 or newer"))?;
            compile_python_wasm_process(&path, &python, &world, verbose).await?;
        } else if is_js_process {
            let valid_node = get_newest_valid_node_version(None, None)?;
            compile_javascript_wasm_process(&path, valid_node, &world, verbose).await?;
        }
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn fetch_local_built_dependency(
    apis: &mut HashMap<String, Vec<u8>>,
    wasm_paths: &mut HashSet<PathBuf>,
    local_dependency: &Path,
) -> Result<()> {
    for entry in local_dependency.join("api").read_dir()? {
        let entry = entry?;
        let path = entry.path();
        let maybe_ext = path.extension().and_then(|s| s.to_str());
        if Some("wit") == maybe_ext {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let wit_contents = fs::read(&path)?;
            apis.insert(file_name.into(), wit_contents);
        }
    }
    for entry in local_dependency.join("target").join("api").read_dir()? {
        let entry = entry?;
        let path = entry.path();
        let maybe_ext = path.extension().and_then(|s| s.to_str());
        if Some("wasm") == maybe_ext {
            wasm_paths.insert(path);
        }
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn fetch_dependencies(
    package_dir: &Path,
    dependencies: &Vec<String>,
    apis: &mut HashMap<String, Vec<u8>>,
    wasm_paths: &mut HashSet<PathBuf>,
    url: Option<String>,
    download_from: Option<&str>,
    mut local_dependencies: Vec<PathBuf>,
    features: &str,
    default_world: Option<&str>,
    force: bool,
    verbose: bool,
) -> Result<()> {
    if let Err(e) = Box::pin(execute(
        package_dir,
        true,
        false,
        true,
        features,
        url.clone(),
        download_from,
        default_world,
        vec![],  // TODO: what about deps-of-deps?
        vec![],
        force,
        verbose,
        true,
    )).await {
        debug!("Failed to build self as dependency: {e:?}");
    } else  if let Err(e) = fetch_local_built_dependency(
        apis,
        wasm_paths,
        package_dir,
    ) {
        debug!("Failed to fetch self as dependency: {e:?}");
    };
    for local_dependency in &local_dependencies {
        // build dependency
        Box::pin(execute(
            local_dependency,
            true,
            false,
            true,
            features,
            url.clone(),
            download_from,
            default_world,
            vec![],  // TODO: what about deps-of-deps?
            vec![],
            force,
            verbose,
            false,
        )).await?;
        fetch_local_built_dependency(apis, wasm_paths, &local_dependency)?;
    }
    let Some(ref url) = url else {
        return Ok(());
    };
    local_dependencies.push(package_dir.into());
    let local_dependencies: HashSet<&str> = local_dependencies
        .iter()
        .map(|p| p.file_name().and_then(|f| f.to_str()).unwrap())
        .collect();
    for dependency in dependencies {
        let Ok(dep) = dependency.parse::<PackageId>() else {
            return Err(eyre!(
                "Dependencies must be PackageIds (e.g. `package:publisher.os`); given {dependency}.",
            ));
        };
        if local_dependencies.contains(dep.package()) {
            continue;
        }
        let Some(zip_dir) = view_api::execute(
            None,
            Some(dependency),
            url,
            download_from,
            false,
        ).await? else {
            return Err(eyre!(
                "Got unexpected result from fetching API for {dependency}"
            ));
        };
        for entry in zip_dir.read_dir()? {
            let entry = entry?;
            let path = entry.path();
            let maybe_ext = path.extension().and_then(|s| s.to_str());
            if Some("wit") == maybe_ext {
                let file_name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                let wit_contents = fs::read(&path)?;
                apis.insert(file_name.into(), wit_contents);
            } else if Some("wasm") == maybe_ext {
                wasm_paths.insert(path);
            }
        }
    }
    Ok(())
}

fn extract_imports_exports_from_wit(input: &str) -> (Vec<String>, Vec<String>) {
    let import_re = regex::Regex::new(r"import\s+([^\s;]+)").unwrap();
    let export_re = regex::Regex::new(r"export\s+([^\s;]+)").unwrap();
    let imports: Vec<String> = import_re.captures_iter(input)
        .map(|cap| cap[1].to_string())
        .filter(|s| !(s.contains("wasi") || s.contains("kinode:process/standard")))
        .collect();

    let exports: Vec<String> = export_re.captures_iter(input)
        .map(|cap| cap[1].to_string())
        .filter(|s| !s.contains("init"))
        .collect();

    (imports, exports)
}

#[instrument(level = "trace", skip_all)]
fn get_imports_exports_from_wasm(
    path: &PathBuf,
    imports: &mut HashMap<String, Vec<PathBuf>>,
    exports: &mut HashMap<String, PathBuf>,
    should_move_export: bool,
) -> Result<()> {
    let wit = run_command(
        Command::new("wasm-tools")
            .args(["component", "wit", path.to_str().unwrap()]),
        false,
    )?;
    let Some((ref wit, _)) = wit else {
        return Ok(());
    };
    let (wit_imports, wit_exports) = extract_imports_exports_from_wit(wit);
    for wit_import in wit_imports {
        imports
            .entry(wit_import)
            .or_insert_with(Vec::new)
            .push(path.clone());
    }
    for wit_export in wit_exports {
        if exports.contains_key(&wit_export) {
            warn!("found multiple exporters of {wit_export}: {path:?} & {exports:?}");
        }
        let path = if should_move_export {
            let file_name = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap()
                .replace("_", "-");
            let new_path = path
                .parent()
                .and_then(|p| p.parent())
                .unwrap()
                .join("target")
                .join("api")
                .join(file_name);
            fs::rename(&path, &new_path)?;
            new_path
        } else {
            path.clone()
        };

        exports.insert(wit_export, path);
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn find_non_standard(
    package_dir: &Path,
    wasm_paths: &mut HashSet<PathBuf>,
) -> Result<(
    HashMap<String, Vec<PathBuf>>,
    HashMap<String, PathBuf>,
    HashSet<PathBuf>,
)> {
    let mut imports = HashMap::new();
    let mut exports = HashMap::new();

    for entry in package_dir.join("pkg").read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if wasm_paths.contains(&path) {
            continue;
        }
        if !(path.is_file() && Some("wasm") == path.extension().and_then(|e| e.to_str())) {
            continue;
        }
        get_imports_exports_from_wasm(&path, &mut imports, &mut exports, true)?;
    }
    for export_path in exports.values() {
        if wasm_paths.contains(export_path) {
            // we already have it; don't include it twice
            wasm_paths.remove(export_path);
        }
    }
    for wasm_path in wasm_paths.iter() {
        get_imports_exports_from_wasm(wasm_path, &mut imports, &mut exports, false)?;
    }

    let others = wasm_paths
        .difference(&exports.values().map(|p| p.clone()).collect())
        .map(|p| p.clone())
        .collect();
    Ok((imports, exports, others))
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
    default_world: Option<&str>,
    download_from: Option<&str>,
    local_dependencies: Vec<PathBuf>,
    add_paths_to_api: Vec<PathBuf>,
    force: bool,
    verbose: bool,
    ignore_deps: bool,
) -> Result<()> {
    let metadata = read_metadata(package_dir)?;
    let mut checked_rust = false;
    let mut checked_py = false;
    let mut checked_js = false;
    let mut apis = HashMap::new();
    let mut wasm_paths = HashSet::new();
    let mut dependencies = HashSet::new();
    for entry in package_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if path.join(RUST_SRC_PATH).exists() && !checked_rust && !skip_deps_check {
                let deps = check_rust_deps()?;
                get_deps(deps, verbose)?;
                checked_rust = true;
            } else if path.join(PYTHON_SRC_PATH).exists() && !checked_py {
                check_py_deps()?;
                checked_py = true;
            } else if path.join(JAVASCRIPT_SRC_PATH).exists() && !checked_js && !skip_deps_check {
                let deps = check_js_deps()?;
                get_deps(deps, verbose)?;
                checked_js = true;
            } else if Some("api") == path.file_name().and_then(|s| s.to_str()) {
                // read api files: to be used in build
                for entry in path.read_dir()? {
                    let entry = entry?;
                    let path = entry.path();
                    if Some("wit") != path.extension().and_then(|e| e.to_str()) {
                        continue;
                    };
                    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    // TODO: reenable check once deps are working
                    // if file_name.starts_with(&format!(
                    //     "{}:{}",
                    //     metadata.properties.package_name,
                    //     metadata.properties.publisher,
                    // )) {
                    //     if let Ok(api_contents) = fs::read(&path) {
                    //         apis.insert(file_name.to_string(), api_contents);
                    //     }
                    // }
                    if let Ok(api_contents) = fs::read(&path) {
                        apis.insert(file_name.to_string(), api_contents);
                    }
                }

                // fetch dependency apis: to be used in build
                if let Some(ref deps) = metadata.properties.dependencies {
                    dependencies.extend(deps);
                }
            }
        }
    }

    if !ignore_deps && !dependencies.is_empty() {
        fetch_dependencies(
            package_dir,
            &dependencies.iter().map(|s| s.to_string()).collect(),
            &mut apis,
            &mut wasm_paths,
            url.clone(),
            download_from,
            local_dependencies.clone(),
            features,
            default_world,
            force,
            verbose,
        ).await?;
    }

    let wit_world = default_world.unwrap_or_else(|| match metadata.properties.wit_version {
        None => DEFAULT_WORLD_0_7_0,
        Some(0) | _ => DEFAULT_WORLD_0_8_0,
    }).to_string();

    let mut tasks = tokio::task::JoinSet::new();
    let features = features.to_string();
    for entry in package_dir.read_dir()? {
        tasks.spawn(compile_package_item(
            entry,
            features.clone(),
            apis.clone(),
            wit_world.clone(),
            metadata.properties.wit_version,
            verbose.clone(),
        ));
    }
    while let Some(res) = tasks.join_next().await {
        res??;
    }

    // create a target/api/ dir: this will be zipped & published in pkg/
    //  In addition, exporters, below, will be placed here to complete the API
    let api_dir = package_dir.join("api");
    let target_api_dir = package_dir.join("target").join("api");
    if api_dir.exists() {
        copy_dir(&api_dir, &target_api_dir)?;
    } else if !target_api_dir.exists() {
        fs::create_dir_all(&target_api_dir)?;
    }

    if !ignore_deps {
        // find non-standard imports/exports -> compositions
        let (importers, exporters, others) = find_non_standard(package_dir, &mut wasm_paths)?;

        // compose
        for (import, import_paths) in importers {
            let Some(export_path) = exporters.get(&import) else {
                return Err(eyre!(
                    "Processes {import_paths:?} required export {import} not found in `pkg/`.",
                ));
            };
            let export_path = export_path.to_str().unwrap();
            for import_path in import_paths {
                let import_path_str = import_path.to_str().unwrap();
                run_command(
                    Command::new("wasm-tools")
                        .args([
                            "compose",
                            import_path_str,
                            "-d",
                            export_path,
                            "-o",
                            import_path_str,
                        ]),
                    false,
                )?;
            }
        }

        // copy others into pkg/
        for path in &others {
            fs::copy(
                path,
                package_dir
                    .join("pkg")
                    .join(path.file_name().and_then(|f| f.to_str()).unwrap())
            )?;
        }
    }

    // zip & place API inside of pkg/ to publish API
    if target_api_dir.exists() {
        for path in add_paths_to_api {
            let path = if path.exists() {
                path
            } else {
                package_dir.join(path).canonicalize().unwrap_or_default()
            };
            if !path.exists() {
                warn!("Given path to add to API does not exist: {path:?}");
                continue;
            }
            if let Err(e) = fs::copy(
                &path,
                target_api_dir.join(path.file_name().and_then(|f| f.to_str()).unwrap()),
            ) {
                warn!("Could not add path {path:?} to API: {e:?}");
            }
        }

        let zip_path = package_dir.join("pkg").join("api.zip");
        let zip_path = zip_path.to_str().unwrap();
        zip_directory(&target_api_dir, zip_path)?;
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
    download_from: Option<&str>,
    default_world: Option<&str>,
    local_dependencies: Vec<PathBuf>,
    add_paths_to_api: Vec<PathBuf>,
    force: bool,
    verbose: bool,
    ignore_deps: bool,  // for internal use; may cause problems when adding recursive deps
) -> Result<()> {
    if !package_dir.join("pkg").exists() {
        if Some(".DS_Store") == package_dir.file_name().and_then(|s| s.to_str()) {
            info!("Skipping build of {:?}", package_dir);
            return Ok(());
        }
        return Err(eyre!(
            "Required `pkg/` dir not found within given input dir {:?} (or cwd, if none given).",
            package_dir,
        )
        .with_suggestion(|| "Please re-run targeting a package."));
    }
    let build_with_features_path = package_dir.join("target").join("build_with_features.txt");
    if !force {
        let old_features = fs::read_to_string(&build_with_features_path).ok();
        if old_features == Some(features.to_string())
            && package_dir.join("Cargo.lock").exists()
            && package_dir.join("pkg").exists()
            && package_dir.join("pkg").join("api.zip").exists()
            && file_with_extension_exists(&package_dir.join("pkg"), "wasm")
        {
            let (source_time, build_time) = get_most_recent_modified_time(
                package_dir,
                &HashSet::from(["Cargo.lock", "api.zip"]),
                &HashSet::from(["wasm"]),
                &HashSet::from(["target"]),
            )?;
            if let Some(source_time) = source_time {
                if let Some(build_time) = build_time {
                    if build_time.duration_since(source_time).is_ok() {
                        // build_time - source_time >= 0
                        //  -> current build is up-to-date: don't rebuild
                        info!("Build up-to-date.");
                        return Ok(());
                    }
                }
            }
        }
    }
    fs::create_dir_all(package_dir.join("target"))?;
    fs::write(&build_with_features_path, features)?;

    check_process_lib_version(&package_dir.join("Cargo.toml"))?;

    let ui_dir = package_dir.join("ui");
    if !ui_dir.exists() {
        if ui_only {
            return Err(eyre!("kit build: can't build UI: no ui directory exists"));
        } else {
            compile_package(
                package_dir,
                skip_deps_check,
                features,
                url,
                default_world.clone(),
                download_from,
                local_dependencies,
                add_paths_to_api,
                force,
                verbose,
                ignore_deps,
            )
            .await?;
        }
    } else {
        if no_ui {
            compile_package(
                package_dir,
                skip_deps_check,
                features,
                url,
                default_world,
                download_from,
                local_dependencies,
                add_paths_to_api,
                force,
                verbose,
                ignore_deps,
            )
            .await?;
        } else {
            let deps = check_js_deps()?;
            get_deps(deps, verbose)?;
            let valid_node = get_newest_valid_node_version(None, None)?;

            if ui_only {
                compile_and_copy_ui(package_dir, valid_node, verbose).await?;
            } else {
                compile_package_and_ui(
                    package_dir,
                    valid_node,
                    skip_deps_check,
                    features,
                    url,
                    default_world,
                    download_from,
                    local_dependencies,
                    add_paths_to_api,
                    force,
                    verbose,
                    ignore_deps,
                )
                .await?;
            }
        }
    }

    let metadata = read_metadata(package_dir)?;
    let pkg_publisher = make_pkg_publisher(&metadata);
    let (_zip_filename, hash_string) = zip_pkg(package_dir, &pkg_publisher)?;
    info!("package zip hash: {hash_string}");

    Ok(())
}
