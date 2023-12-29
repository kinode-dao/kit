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

fn check_and_install_nvm() -> anyhow::Result<()> {
    if !is_nvm_installed()? {
        install_nvm()?;
    } else {
        println!("Found nvm.");
    }
    Ok(())
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
        .stdout(Stdio::null())
    )?;

    println!("Done getting nvm.");
    Ok(())
}

fn check_and_install_node() -> anyhow::Result<()> {
    if !is_command_installed("node")? || !is_version_correct("node", (REQUIRED_NODE_MAJOR, MINIMUM_NODE_MINOR))? {
        let node_version = format!("{}.{}", REQUIRED_NODE_MAJOR, MINIMUM_NODE_MINOR);
        println!("Installing or updating Node.js to version {}...", node_version);
        call_nvm(&format!("install {}", node_version))?;
        println!("Done installing or updating Node.js to version {}.", node_version);
    } else {
        println!("Found node.");
    }
    Ok(())
}

fn check_and_install_npm() -> anyhow::Result<()> {
    if !is_command_installed("npm")? || !is_version_correct("npm", (REQUIRED_NPM_MAJOR, MINIMUM_NODE_MINOR))? {
        let npm_version = format!("{}.{}", REQUIRED_NPM_MAJOR, MINIMUM_NPM_MINOR);
        println!("Installing or updating npm to version {}...", npm_version);
        call_nvm(&format!("install-latest-npm"))?;
        println!("Done installing or updating npm to version {}...", npm_version);
    } else {
        println!("Found npm.");
    }
    Ok(())
}

fn check_python_venv(python: &str) -> anyhow::Result<()> {
    println!("Testing python venv capability...");
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
            println!("Done testing python venv capability.");
            Ok(())
        },
        Err(_) => Err(anyhow::anyhow!("Done testing python venv capability: could not create python venv.")),
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
    )?;
    Ok(())
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

pub fn execute() -> anyhow::Result<()> {
    println!("Setting up...");
    let python = get_python_version(Some(REQUIRED_PY_MAJOR), Some(MINIMUM_PY_MINOR))?
        .ok_or(anyhow::anyhow!("uqdev requires Python 3.10 or newer"))?;
    // If setup required, request user permission
    print!("Do you want to check Uqdev dependencies and install any that are not found (nvm, npm, node, componentize-py)? [Y/n]: ");
    // Flush to ensure the prompt is displayed before input
    io::stdout().flush().unwrap();

    // Read the user's response
    let mut response = String::new();
    io::stdin().read_line(&mut response).unwrap();

    // Process the response
    let response = response.trim().to_lowercase(); // Normalize the input
    match response.as_str() {
        "y" | "yes" | "" => {
            check_and_install_nvm()?;
            check_and_install_node()?;
            check_and_install_npm()?;
            check_python_venv(&python)?;
            println!("Done setting up.");
        },
        _ => println!("Skipped setting up."),
    }
    Ok(())
}
