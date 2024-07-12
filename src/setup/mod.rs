use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str;

use color_eyre::{eyre::eyre, Result};
use fs_err as fs;
use tracing::{info, instrument, warn};

use crate::build::run_command;

const FETCH_NVM_VERSION: &str = "v0.39.7";
const REQUIRED_NODE_MAJOR: u32 = 20;
const MINIMUM_NODE_MINOR: u32 = 0;
const REQUIRED_NPM_MAJOR: u32 = 9;
const MINIMUM_NPM_MINOR: u32 = 0;
pub const REQUIRED_PY_MAJOR: u32 = 3;
pub const MINIMUM_PY_MINOR: u32 = 10;
pub const REQUIRED_PY_PACKAGE: &str = "componentize-py==0.11.0";

#[derive(Clone)]
pub enum Dependency {
    Anvil,
    Forge,
    Nvm,
    Npm,
    Node,
    Rust,
    RustNightly,
    RustWasm32Wasi,
    RustNightlyWasm32Wasi,
    WasmTools,
}

impl std::fmt::Display for Dependency {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Dependency::Anvil => write!(f, "anvil"),
            Dependency::Forge => write!(f, "forge"),
            Dependency::Nvm => write!(f, "nvm {}", FETCH_NVM_VERSION),
            Dependency::Npm => write!(f, "npm {}.{}", REQUIRED_NPM_MAJOR, MINIMUM_NPM_MINOR),
            Dependency::Node => write!(f, "node {}.{}", REQUIRED_NODE_MAJOR, MINIMUM_NODE_MINOR),
            Dependency::Rust => write!(f, "rust"),
            Dependency::RustNightly => write!(f, "rust nightly"),
            Dependency::RustWasm32Wasi => write!(f, "rust wasm32-wasi target"),
            Dependency::RustNightlyWasm32Wasi => write!(f, "rust nightly wasm32-wasi target"),
            Dependency::WasmTools => write!(f, "wasm-tools"),
        }
    }
}

// hack to allow definition of Display
struct Dependencies(Vec<Dependency>);
impl std::fmt::Display for Dependencies {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let deps: Vec<String> = self.0.iter().map(|d| d.to_string()).collect();
        write!(f, "{}", deps.join(", "))
    }
}

#[instrument(level = "trace", skip_all)]
fn is_nvm_installed() -> Result<bool> {
    let home_dir = env::var("HOME")?;
    let nvm_dir = format!("{}/.nvm", home_dir);
    Ok(std::path::Path::new(&nvm_dir).exists())
}

#[instrument(level = "trace", skip_all)]
fn install_nvm(verbose: bool) -> Result<()> {
    info!("Getting nvm...");
    let install_nvm = format!(
        "curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/{}/install.sh | bash",
        FETCH_NVM_VERSION,
    );
    run_command(Command::new("bash").args(&["-c", &install_nvm]), verbose)?;

    info!("Done getting nvm.");
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn install_rust(verbose: bool) -> Result<()> {
    info!("Getting rust...");
    let install_rust = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh";
    run_command(Command::new("bash").args(&["-c", install_rust]), verbose)?;

    info!("Done getting rust.");
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn check_python_venv(python: &str) -> Result<()> {
    info!("Checking for python venv...");
    let venv_result = run_command(
        Command::new(python)
            .args(&["-m", "venv", "kinode-test-venv"])
            .current_dir("/tmp"),
        false,
    );
    let venv_dir = PathBuf::from("/tmp/kinode-test-venv");
    if venv_dir.exists() {
        fs::remove_dir_all(&venv_dir)?;
    }
    match venv_result {
        Ok(_) => {
            info!("Found python venv.");
            Ok(())
        }
        Err(_) => Err(eyre!("Check for python venv failed.")),
    }
}

#[instrument(level = "trace", skip_all)]
fn is_command_installed(cmd: &str) -> Result<bool> {
    Ok(Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .status()?
        .success())
}

#[instrument(level = "trace", skip_all)]
fn is_npm_version_correct(node_version: String, required_version: (u32, u32)) -> Result<bool> {
    let version = call_with_nvm_output(&format!("nvm use {node_version} && npm --version"))?;
    let version = version
        .split('\n')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>();
    let version = version.last().unwrap_or_else(|| &"");
    Ok(parse_version(version)
        .and_then(|v| Some(compare_versions(v, required_version)))
        .unwrap_or(false))
}

#[instrument(level = "trace", skip_all)]
fn is_version_correct(cmd: &str, required_version: (u32, u32)) -> Result<bool> {
    let output = Command::new(cmd).arg("--version").output()?.stdout;

    let version = String::from_utf8_lossy(&output);

    Ok(parse_version(version.trim())
        .and_then(|v| Some(compare_versions(v, required_version)))
        .unwrap_or(false))
}

fn strip_color_codes(input: &str) -> String {
    let re = regex::Regex::new("\x1b\\[[^m]*m").unwrap();
    re.replace_all(input, "").into_owned()
}

/// Get the newest valid `node` via `nvm`, provided
/// that version is at least as new as `required_major`.
///
/// Returns `None` if no valid version; `Some(String)`:
/// the valid version, as a String.
#[instrument(level = "trace", skip_all)]
pub fn get_newest_valid_node_version(
    required_major: Option<u32>,
    minimum_minor: Option<u32>,
) -> Result<Option<String>> {
    let required_major = required_major.unwrap_or(REQUIRED_NODE_MAJOR);
    let minimum_minor = minimum_minor.unwrap_or(MINIMUM_NODE_MINOR);

    let output = Command::new("bash")
        .arg("-c")
        .arg("source ~/.nvm/nvm.sh && nvm ls --no-alias")
        .output()?
        .stdout;

    let nvm_ls = String::from_utf8_lossy(&output);
    let mut versions = Vec::new();

    for line in nvm_ls.lines() {
        let line = strip_color_codes(line);
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() == 1 {
            versions.push(fields[0].to_string());
        } else if fields.len() == 2 && fields[0] == "->" {
            versions.push(fields[1].to_string());
        }
    }

    let mut newest_node = None;
    let mut max_version = (0, 0); // (major, minor)

    for version in versions {
        if let Some((major, minor)) = parse_version(&version) {
            if major == required_major && minor >= minimum_minor && (major, minor) > max_version {
                max_version = (major, minor);
                newest_node = Some(version.to_string());
            }
        }
    }

    Ok(newest_node)
}

#[instrument(level = "trace", skip_all)]
fn call_with_nvm_output(arg: &str) -> Result<String> {
    let output = Command::new("bash")
        .args(&["-c", &format!("source ~/.nvm/nvm.sh && {}", arg)])
        .output()?
        .stdout;
    Ok(String::from_utf8_lossy(&output).to_string())
}

#[instrument(level = "trace", skip_all)]
fn call_with_nvm(arg: &str, verbose: bool) -> Result<()> {
    run_command(
        Command::new("bash").args(&["-c", &format!("source ~/.nvm/nvm.sh && {}", arg)]),
        verbose,
    )?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn call_rustup(arg: &str, verbose: bool) -> Result<()> {
    run_command(
        Command::new("bash").args(&["-c", &format!("rustup {}", arg)]),
        verbose,
    )?;
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn call_cargo(arg: &str, verbose: bool) -> Result<()> {
    let command = if arg.contains("--color=always") {
        format!("cargo {}", arg)
    } else {
        format!("cargo --color=always {}", arg)
    };
    run_command(Command::new("bash").args(&["-c", &command]), verbose)?;
    Ok(())
}

fn compare_versions(installed_version: (u32, u32), required_version: (u32, u32)) -> bool {
    installed_version.0 == required_version.0 && installed_version.1 >= required_version.1
}

fn parse_version(version_str: &str) -> Option<(u32, u32)> {
    let mut parts: Vec<&str> = version_str.split('.').collect();

    if parts.is_empty() {
        return None;
    }

    // Remove leading 'v' from the first part if present
    parts[0] = parts[0].trim_start_matches('v');

    if parts.len() >= 2 {
        if let (Ok(major), Ok(minor)) = (parts[0].parse(), parts[1].parse()) {
            return Some((major, minor));
        }
    }

    None
}

#[instrument(level = "trace", skip_all)]
fn check_rust_toolchains_targets() -> Result<Vec<Dependency>> {
    let mut missing_deps = Vec::new();
    let output = Command::new("rustup").arg("show").output()?.stdout;
    let output = String::from_utf8_lossy(&output);
    let original_default = output.split('\n').fold("", |d, item| {
        if !item.contains("(default)") {
            d
        } else {
            item.split(' ').nth(0).unwrap_or("")
        }
    });

    // check stable deps
    run_command(
        Command::new("rustup")
            .args(&["default", "stable"])
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        false,
    )?;
    let output = Command::new("rustup").arg("show").output()?.stdout;
    let output = String::from_utf8_lossy(&output);

    let has_wasm32_wasi = output
        .split('\n')
        .fold(false, |acc, item| acc || item == "wasm32-wasi");
    if !has_wasm32_wasi {
        missing_deps.push(Dependency::RustWasm32Wasi);
    }

    // check nightly deps
    let has_nightly_toolchain = output
        .split('\n')
        .fold(false, |acc, item| acc || item.starts_with("nightly"));
    if !has_nightly_toolchain {
        missing_deps.append(&mut vec![
            Dependency::RustNightly,
            Dependency::RustNightlyWasm32Wasi,
        ]);
    } else {
        // check for nightly wasm32-wasi
        run_command(
            Command::new("rustup")
                .args(&["default", "nightly"])
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
            false,
        )?;
        let output = Command::new("rustup").arg("show").output()?.stdout;
        let output = String::from_utf8_lossy(&output);

        let has_wasm32_wasi = output
            .split('\n')
            .fold(false, |acc, item| acc || item == "wasm32-wasi");
        if !has_wasm32_wasi {
            missing_deps.push(Dependency::RustNightlyWasm32Wasi);
        }
    }

    run_command(
        Command::new("rustup")
            .args(&["default", original_default])
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        false,
    )?;
    Ok(missing_deps)
}

/// Find the newest Python version (>= 3.10 or given major, minor)
#[instrument(level = "trace", skip_all)]
pub fn get_python_version(
    required_major: Option<u32>,
    minimum_minor: Option<u32>,
) -> Result<Option<String>> {
    let required_major = required_major.unwrap_or(REQUIRED_PY_MAJOR);
    let minimum_minor = minimum_minor.unwrap_or(MINIMUM_PY_MINOR);
    let output = Command::new("bash")
        .arg("-c")
        .arg("for dir in $(echo $PATH | tr ':' ' '); do for cmd in $(echo $dir/python3*); do which $(basename $cmd) 2>/dev/null; done; done")
        .output()?;

    let commands = str::from_utf8(&output.stdout)?;
    let python_versions = commands.split_whitespace();

    let mut newest_python = None;
    let mut max_version = (0, 0); // (major, minor)

    for python in python_versions {
        let version_output = Command::new(python).arg("--version").output()?;

        let version_str = str::from_utf8(&version_output.stdout).unwrap_or("");
        if version_str.is_empty() {
            continue;
        }

        if let Some(version) = version_str.split_whitespace().nth(1) {
            if let Some((major, minor)) = parse_version(version) {
                if major == required_major && minor >= minimum_minor && (major, minor) > max_version
                {
                    max_version = (major, minor);
                    newest_python = Some(python.to_string());
                }
            }
        }
    }

    Ok(newest_python)
}

/// Check for Python deps, erroring if not found: python deps cannot be automatically fetched
#[instrument(level = "trace", skip_all)]
pub fn check_py_deps() -> Result<String> {
    let python = get_python_version(Some(REQUIRED_PY_MAJOR), Some(MINIMUM_PY_MINOR))?
        .ok_or_else(|| eyre!("kit requires Python 3.10 or newer"))?;
    check_python_venv(&python)?;

    Ok(python)
}

/// Check for Javascript deps, returning a Vec of not found: can be automatically fetched
#[instrument(level = "trace", skip_all)]
pub fn check_js_deps() -> Result<Vec<Dependency>> {
    let mut missing_deps = Vec::new();
    if !is_nvm_installed()? {
        missing_deps.push(Dependency::Nvm);
    }
    let valid_node = get_newest_valid_node_version(None, None)?;
    match valid_node {
        None => missing_deps.extend_from_slice(&[Dependency::Node, Dependency::Npm]),
        Some(vn) => {
            if !is_command_installed("npm")?
                || !is_npm_version_correct(vn, (REQUIRED_NPM_MAJOR, MINIMUM_NPM_MINOR))?
            {
                missing_deps.push(Dependency::Npm);
            }
        }
    }
    Ok(missing_deps)
}

// Check for Foundry deps, returning a Vec of not found: can be automatically fetched?
#[instrument(level = "trace", skip_all)]
pub fn check_foundry_deps() -> Result<Vec<Dependency>> {
    let mut missing_deps = Vec::new();
    if !is_command_installed("forge")? {
        missing_deps.push(Dependency::Forge);
    }
    if !is_command_installed("anvil")? {
        missing_deps.push(Dependency::Anvil);
    }
    Ok(missing_deps)
}

// install forge+anvil+others, could be separated into binary extractions from github releases.
pub fn install_foundry() -> Result<()> {
    let install_cmd = "curl -L https://foundry.paradigm.xyz | bash";
    run_command(Command::new("bash").args(&["-c", install_cmd]), false)?;

    Ok(())
}
/// Check for Rust deps, returning a Vec of not found: can be automatically fetched
#[instrument(level = "trace", skip_all)]
pub fn check_rust_deps() -> Result<Vec<Dependency>> {
    if !is_command_installed("rustup")? {
        // don't have rust -> missing all
        return Ok(vec![
            Dependency::Rust,
            Dependency::RustNightly,
            Dependency::RustWasm32Wasi,
            Dependency::RustNightlyWasm32Wasi,
            Dependency::WasmTools,
        ]);
    }

    let mut missing_deps = check_rust_toolchains_targets()?;
    if !is_command_installed("wasm-tools")? {
        missing_deps.push(Dependency::WasmTools);
    }

    Ok(missing_deps)
}

#[instrument(level = "trace", skip_all)]
pub fn get_deps(deps: Vec<Dependency>, verbose: bool) -> Result<()> {
    if deps.is_empty() {
        return Ok(());
    }

    // If setup required, request user permission
    print!(
        "kit requires {} missing {}: {}. Install? [Y/n]: ",
        if deps.len() == 1 { "this" } else { "these" },
        if deps.len() == 1 {
            "dependency"
        } else {
            "dependencies"
        },
        Dependencies(deps.clone()),
    );
    // Flush to ensure the prompt is displayed before input
    io::stdout().flush().unwrap();

    // Read the user's response
    let mut response = String::new();
    io::stdin().read_line(&mut response).unwrap();

    // Process the response
    let response = response.trim().to_lowercase();
    match response.as_str() {
        "y" | "yes" | "" => {
            for dep in deps {
                match dep {
                    Dependency::Nvm => install_nvm(verbose)?,
                    Dependency::Npm => call_with_nvm(&format!("nvm install-latest-npm"), verbose)?,
                    Dependency::Node => call_with_nvm(
                        &format!("nvm install {}.{}", REQUIRED_NODE_MAJOR, MINIMUM_NODE_MINOR,),
                        verbose,
                    )?,
                    Dependency::Rust => install_rust(verbose)?,
                    Dependency::RustNightly => call_rustup("install nightly", verbose)?,
                    Dependency::RustWasm32Wasi => call_rustup("target add wasm32-wasi", verbose)?,
                    Dependency::RustNightlyWasm32Wasi => {
                        call_rustup("target add wasm32-wasi --toolchain nightly", verbose)?
                    }
                    Dependency::WasmTools => call_cargo("install wasm-tools", verbose)?,
                    Dependency::Forge | Dependency::Anvil => install_foundry()?,
                }
            }
        }
        r => warn!("Got '{}'; not getting deps.", r),
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
pub fn execute(verbose: bool) -> Result<()> {
    info!("Setting up...");

    check_py_deps()?;
    let mut missing_deps = check_js_deps()?;
    missing_deps.append(&mut check_rust_deps()?);
    get_deps(missing_deps, verbose)?;

    info!("Done setting up.");
    Ok(())
}
