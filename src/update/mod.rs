use std::process::Command;

use super::build::run_command;

pub fn execute() -> anyhow::Result<()> {
    run_command(Command::new("cargo")
        .args(&["install", "--git", "https://github.com/uqbar-dao/uqdev", "--branch", "next-release"])
        //.args(&["install", "--git", "https://github.com/uqbar-dao/uqdev"]) // TODO
    )?;
    Ok(())
}
