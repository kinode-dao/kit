use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str;

use super::build::run_command;

const FETCH_NVM_VERSION: &str = "v0.39.7";
const REQUIRED_NODE_MAJOR: u32 = 18;
const MINIMUM_NODE_MINOR: u32 = 0;
const REQUIRED_NPM_MAJOR: u32 = 9;
const MINIMUM_NPM_MINOR: u32 = 0;
pub const REQUIRED_PY_MAJOR: u32 = 3;
pub const MINIMUM_PY_MINOR: u32 = 10;
pub const REQUIRED_PY_PACKAGE: &str = "componentize-py==0.7.1";

#[derive(Clone)]
pub enum Dependency {
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
            Dependency::Nvm =>  write!(f, "nvm"),
            Dependency::Npm =>  write!(f, "npm"),
            Dependency::Node => write!(f, "node"),
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

fn is_nvm_installed() -> anyhow::Result<bool> {
    let home_dir = env::var("HOME")?;
    let nvm_dir = format!("{}/.nvm", home_dir);
    Ok(std::path::Path::new(&nvm_dir).exists())
}

fn install_nvm() -> anyhow::Result<()> {
    println!("Getting nvm...");
    let install_script = format!(
        "https://raw.githubusercontent.com/nvm-sh/nvm/{FETCH_NVM_VERSION}/install.sh"
    );
    run_command(Command::new("bash")
        .args(&["-c", &format!("curl -o- {install_script} | bash")])
    )?;

    println!("Done getting nvm.");
    Ok(())
}

fn install_rust() -> anyhow::Result<()> {
    println!("Getting rust...");
    let install_rust = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh";
    run_command(Command::new("bash")
        .args(&["-c", install_rust])
    )?;

    println!("Done getting rust.");
    Ok(())
}

fn check_python_venv(python: &str) -> anyhow::Result<()> {
    println!("Checking for python venv...");
    let venv_result = run_command(Command::new(python)
        .args(&["-m", "venv", "uqbar-test-venv"])
        .current_dir("/tmp")
    );
    let venv_dir = PathBuf::from("/tmp/uqbar-test-venv");
    if venv_dir.exists() {
        std::fs::remove_dir_all(&venv_dir)?;
    }
    match venv_result {
        Ok(_) => {
            println!("Found python venv.");
            Ok(())
        },
        Err(_) => Err(anyhow::anyhow!("Check for python venv failed.")),
    }
}

fn is_command_installed(cmd: &str) -> anyhow::Result<bool> {
    Ok(Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .status()?
        .success()
    )
}

fn is_version_correct(cmd: &str, required_version: (u32, u32)) -> anyhow::Result<bool> {
    let output = Command::new(cmd)
        .arg("--version")
        .output()?
        .stdout;

    let version = String::from_utf8_lossy(&output);

    Ok(parse_version(version.trim())
        .and_then(|v| Some(compare_versions(v, required_version)))
        .unwrap_or(false)
    )
}

fn call_nvm(arg: &str) -> anyhow::Result<()> {
    run_command(Command::new("bash")
        .arg("-c")
        .arg(format!("source ~/.nvm/nvm.sh && nvm {}", arg))
    )
}

fn call_rustup(arg: &str) -> anyhow::Result<()> {
    run_command(Command::new("bash")
        .arg("-c")
        .arg(format!("rustup {}", arg))
    )
}

fn call_cargo(arg: &str) -> anyhow::Result<()> {
    run_command(Command::new("bash")
        .arg("-c")
        .arg(format!("cargo {}", arg))
    )
}

fn compare_versions(installed_version: (u32, u32) , required_version: (u32, u32)) -> bool {
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

fn check_rust_toolchains_targets() -> anyhow::Result<Vec<Dependency>> {
    let mut missing_deps = Vec::new();
    let output = Command::new("rustup")
        .arg("show")
        .output()?
        .stdout;
    let output = String::from_utf8_lossy(&output);
    let original_default = output
        .split('\n')
        .fold("", |d, item| {
            if !item.contains("(default)") {
                d
            } else {
                item.split(' ')
                    .nth(0)
                    .unwrap_or("")
            }
        });

    // check stable deps
    run_command(Command::new("rustup")
        .args(&["default", "stable"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
    )?;
    let output = Command::new("rustup")
        .arg("show")
        .output()?
        .stdout;
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
        run_command(Command::new("rustup")
            .args(&["default", "nightly"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
        )?;
        let output = Command::new("rustup")
            .arg("show")
            .output()?
            .stdout;
        let output = String::from_utf8_lossy(&output);

        let has_wasm32_wasi = output
            .split('\n')
            .fold(false, |acc, item| acc || item == "wasm32-wasi");
        if !has_wasm32_wasi {
            missing_deps.push(Dependency::RustNightlyWasm32Wasi);
        }
    }

    run_command(Command::new("rustup")
        .args(&["default", original_default])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
    )?;
    Ok(missing_deps)
}

/// Find the newest Python version (>= 3.10 or given major, minor)
pub fn get_python_version(
    required_major: Option<u32>,
    minimum_minor: Option<u32>,
) -> anyhow::Result<Option<String>> {
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
        let version_output = Command::new(python)
            .arg("--version")
            .output()?;

        let version_str = str::from_utf8(&version_output.stdout).unwrap_or("");
        if version_str.is_empty() {
            continue;
        }

        if let Some(version) = version_str.split_whitespace().nth(1) {
            if let Some((major, minor)) = parse_version(version) {
                if major == required_major && minor >= minimum_minor && (major, minor) > max_version {
                    max_version = (major, minor);
                    newest_python = Some(python.to_string());
                }
            }
        }
    }

    Ok(newest_python)
}

/// Check for Python deps, erroring if not found: python deps cannot be automatically fetched
pub fn check_py_deps() -> anyhow::Result<String> {
    let python = get_python_version(Some(REQUIRED_PY_MAJOR), Some(MINIMUM_PY_MINOR))?
        .ok_or(anyhow::anyhow!("uqdev requires Python 3.10 or newer"))?;
    check_python_venv(&python)?;

    Ok(python)
}

/// Check for Javascript deps, returning a Vec of not found: can be automatically fetched
pub fn check_js_deps() -> anyhow::Result<Vec<Dependency>> {
    let mut missing_deps = Vec::new();
    if !is_nvm_installed()? {
        missing_deps.push(Dependency::Nvm);
    }
    if !is_command_installed("node")?
    || !is_version_correct("node", (REQUIRED_NODE_MAJOR, MINIMUM_NODE_MINOR))? {
        missing_deps.push(Dependency::Node);
    }
    if !is_command_installed("npm")?
    || !is_version_correct("npm", (REQUIRED_NPM_MAJOR, MINIMUM_NPM_MINOR))? {
        missing_deps.push(Dependency::Npm);
    }

    Ok(missing_deps)
}

/// Check for Rust deps, returning a Vec of not found: can be automatically fetched
pub fn check_rust_deps() -> anyhow::Result<Vec<Dependency>> {
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

pub fn get_deps(deps: Vec<Dependency>) -> anyhow::Result<()> {
    if deps.is_empty() {
        return Ok(());
    }

    // If setup required, request user permission
    print!(
        "UqDev requires {} missing {}: {}. Install? [Y/n]: ",
        if deps.len() == 1 { "this" } else { "these" },
        if deps.len() == 1 { "dependency" } else { "dependencies" },
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
                    Dependency::Nvm =>  install_nvm()?,
                    Dependency::Npm =>  call_nvm(&format!("install-latest-npm"))?,
                    Dependency::Node => {
                        call_nvm(&format!(
                            "install {}.{}",
                            REQUIRED_NODE_MAJOR,
                            MINIMUM_NODE_MINOR,
                        ))?
                    },
                    Dependency::Rust => install_rust()?,
                    Dependency::RustNightly => call_rustup("install nightly")?,
                    Dependency::RustWasm32Wasi => call_rustup("rustup target add wasm32-wasi")?,
                    Dependency::RustNightlyWasm32Wasi => {
                        call_rustup("rustup target add wasm32-wasi --toolchain nightly")?
                    },
                    Dependency::WasmTools => call_cargo("install wasm-tools")?,
                }
            }
        },
        r => println!("Got '{}'; not getting deps.", r),
    }
    Ok(())
}

pub fn execute() -> anyhow::Result<()> {
    println!("Setting up...");

    check_py_deps()?;
    let mut missing_deps = check_js_deps()?;
    missing_deps.append(&mut check_rust_deps()?);
    get_deps(missing_deps)?;

    println!("Done setting up.");
    Ok(())
}
