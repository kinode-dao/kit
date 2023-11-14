use clap::{Arg, ArgAction, command, Command};
use std::env;
use std::path::PathBuf;

mod build;
mod inject_message;
mod start_package;
mod run_tests;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let current_dir = env::current_dir()?.into_os_string();
    // let current_dir = env::current_dir()?.as_os_str();
    // let current_dir: String = env::current_dir()?.to_str().unwrap_or("").to_string();
    let mut app = command!()
        .name("UqDev")
        .version("0.1.0")
        .about("Development tools for Uqbar")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("build")
            .about("Build an Uqbar process")
            .arg(Arg::new("project_dir")
                .action(ArgAction::Set)
                .default_value(&current_dir)
                .help("The project directory to build")
                .required(true)
            )
        )
        .subcommand(Command::new("inject-message")
            .about("Inject a message to a running Uqbar node")
            .arg(Arg::new("url")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .required(true))
            .arg(Arg::new("process")
                .action(ArgAction::Set)
                .short('p')
                .long("process")
                .required(true)
                .help("Process to send message to"))
            .arg(Arg::new("ipc")
                .action(ArgAction::Set)
                .short('i')
                .long("ipc")
                .required(true)
                .help("IPC in JSON format"))
            .arg(Arg::new("node")
                .action(ArgAction::Set)
                .short('n')
                .long("node")
                .required(false)
                .help("Node ID (default: our)"))
            .arg(Arg::new("bytes")
                .action(ArgAction::Set)
                .short('b')
                .long("bytes")
                .required(false)
                .help("Send bytes from path on Unix system"))
        )
        .subcommand(Command::new("start-package")
            .about("Start a built Uqbar process")
            .arg(Arg::new("pkg_dir")
                .action(ArgAction::Set)
                .short('p')
                .long("pkg_dir")
                .required(true))
            .arg(Arg::new("url")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .required(true))
            .arg(Arg::new("node")
                .action(ArgAction::Set)
                .short('n')
                .long("node")
                .required(false))
        )
        .subcommand(Command::new("run-tests")
            .about("Run Uqbar tests")
            .arg(Arg::new("config")
                .action(ArgAction::Set)
                .short('c')
                .long("config")
                .help("Path to tests configuration file")
                .default_value("tests.toml")
            )
        );

    let usage = app.render_usage();
    let matches = app.get_matches();
    let matches = matches.subcommand();

    match matches {
        Some(("build", build_matches)) => {
            let project_dir = PathBuf::from(build_matches.get_one::<String>("project_dir").unwrap());
            build::compile_process(&project_dir)?;
        },
        Some(("inject-message", inject_message_matches)) => {
            let url: &String = inject_message_matches.get_one("url").unwrap();
            let process: &String = inject_message_matches.get_one("process").unwrap();
            let ipc: &String = inject_message_matches.get_one("ipc").unwrap();
            let node: Option<&str> = inject_message_matches
                .get_one("node")
                .and_then(|s: &String| Some(s.as_str()));
            let bytes: Option<&str> = inject_message_matches
                .get_one("bytes")
                .and_then(|s: &String| Some(s.as_str()));
            inject_message::execute(url, process, ipc, node, bytes).await?;
        },
        Some(("start-package", start_package_matches)) => {
            let pkg_dir: &String = start_package_matches.get_one("pkg_dir").unwrap();
            let url: &String = start_package_matches.get_one("url").unwrap();
            let node: Option<&str> = start_package_matches
                .get_one("node")
                .and_then(|s: &String| Some(s.as_str()));
            start_package::execute(pkg_dir, url, node).await?;
        },
        Some(("run-tests", run_tests_matches)) => {
            let config_path = match run_tests_matches.get_one::<String>("config") {
                Some(path) => PathBuf::from(path),
                None => std::env::current_dir()?.join("tests.toml"),
            };

            if !config_path.exists() {
                return Err(format!("Configuration file not found: {:?}", config_path).into());
            }

            run_tests::execute(config_path.to_str().unwrap()).await?;
        },
        _ => println!("Invalid subcommand. Usage:\n{}", usage),
    }

    Ok(())
}
