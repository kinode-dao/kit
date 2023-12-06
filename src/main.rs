use clap::{Arg, ArgAction, command, Command};
use std::env;
use std::path::PathBuf;

mod build;
mod inject_message;
mod new;
mod run_tests;
mod start_package;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
                .help("The project directory to build")
                .default_value(&current_dir)
            )
            .arg(Arg::new("quiet")
                .action(ArgAction::SetTrue)
                .short('q')
                .long("quiet")
                .help("If set, do not print `cargo` stdout/stderr")
                .required(false)
            )
        )
        .subcommand(Command::new("inject-message")
            .about("Inject a message to a running Uqbar node")
            .arg(Arg::new("url")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .required(true)
            )
            .arg(Arg::new("process")
                .action(ArgAction::Set)
                .short('p')
                .long("process")
                .help("Process to send message to")
                .required(true)
            )
            .arg(Arg::new("ipc")
                .action(ArgAction::Set)
                .short('i')
                .long("ipc")
                .help("IPC in JSON format")
                .required(true)
            )
            .arg(Arg::new("node")
                .action(ArgAction::Set)
                .short('n')
                .long("node")
                .help("Node ID (default: our)")
                .required(false)
            )
            .arg(Arg::new("bytes")
                .action(ArgAction::Set)
                .short('b')
                .long("bytes")
                .help("Send bytes from path on Unix system")
                .required(false)
            )
        )
        .subcommand(Command::new("new")
            .about("Create an Uqbar template project")
            .arg(Arg::new("directory")
                .action(ArgAction::Set)
                .help("Path to create template directory at")
                .required(true)
            )
            .arg(Arg::new("package-name")
                .action(ArgAction::Set)
                .short('p')
                .long("package")
                .help("Name of the package")
                .required(true)
            )
        )
        .subcommand(Command::new("run-tests")
            .about("Run Uqbar tests")
            .arg(Arg::new("config")
                .action(ArgAction::Set)
                .help("Path to tests configuration file")
                .default_value("tests.toml")
            )
        )
        .subcommand(Command::new("start-package")
            .about("Start a built Uqbar process")
            .arg(Arg::new("project_dir")
                .action(ArgAction::Set)
                .help("The project directory to build")
                .default_value(&current_dir)
            )
            .arg(Arg::new("url")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .required(true)
            )
            .arg(Arg::new("node")
                .action(ArgAction::Set)
                .short('n')
                .long("node")
                .required(false)
            )
        );

    let usage = app.render_usage();
    let matches = app.get_matches();
    let matches = matches.subcommand();

    match matches {
        Some(("build", build_matches)) => {
            let project_dir = PathBuf::from(build_matches.get_one::<String>("project_dir").unwrap());
            let verbose = !build_matches.get_one::<bool>("quiet").unwrap();
            build::compile_package(&project_dir, verbose).await?;
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
        Some(("new", new_matches)) => {
            let new_dir = PathBuf::from(new_matches.get_one::<String>("directory").unwrap());
            let package_name = new_matches.get_one::<String>("package-name").unwrap();

            new::execute(new_dir, package_name.clone())?;
        },
        Some(("run-tests", run_tests_matches)) => {
            let config_path = match run_tests_matches.get_one::<String>("config") {
                Some(path) => PathBuf::from(path),
                None => std::env::current_dir()?.join("tests.toml"),
            };

            if !config_path.exists() {
                let error = format!(
                    "Configuration file not found: {:?}\nUsage:\n{}",
                    config_path,
                    usage,
                );
                println!("{}", error);
                return Err(anyhow::anyhow!(error));
            }

            run_tests::execute(config_path.to_str().unwrap()).await?;
        },
        Some(("start-package", start_package_matches)) => {
            let project_dir = PathBuf::from(start_package_matches.get_one::<String>("project_dir").unwrap());
            let url: &String = start_package_matches.get_one("url").unwrap();
            let node: Option<&str> = start_package_matches
                .get_one("node")
                .and_then(|s: &String| Some(s.as_str()));
            start_package::execute(project_dir, url, node).await?;
        },
        _ => println!("Invalid subcommand. Usage:\n{}", usage),
    }

    Ok(())
}
