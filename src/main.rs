use clap::{Arg, ArgAction, command, Command, value_parser};
use std::env;
use std::path::PathBuf;

mod boot_fake_node;
mod build;
mod inject_message;
mod new;
mod run_tests;
mod start_package;

async fn execute(
    usage: clap::builder::StyledStr,
    matches: Option<(&str, &clap::ArgMatches)>,
) -> anyhow::Result<()> {
    match matches {
        Some(("boot-fake-node", boot_matches)) => {
            let runtime_path = boot_matches
                .get_one::<String>("runtime-path")
                .and_then(|p| Some(PathBuf::from(p)));
            let version = boot_matches.get_one::<String>("version").unwrap();
            let node_home = PathBuf::from(boot_matches.get_one::<String>("node-home").unwrap());
            let node_port = boot_matches.get_one::<u16>("node-port").unwrap();
            let network_router_port = boot_matches.get_one::<u16>("network-router-port").unwrap();
            let rpc = boot_matches.get_one::<String>("rpc").and_then(|s| Some(s.as_str()));
            let fake_node_name = boot_matches.get_one::<String>("fake-node-name").unwrap();
            let password = boot_matches.get_one::<String>("password").unwrap();

            boot_fake_node::execute(
                runtime_path,
                version.clone(),
                node_home,
                *node_port,
                *network_router_port,
                rpc,
                fake_node_name,
                password,
                vec![],
            ).await
        },
        Some(("build", build_matches)) => {
            let package_dir = PathBuf::from(build_matches.get_one::<String>("package-dir").unwrap());
            let verbose = !build_matches.get_one::<bool>("quiet").unwrap();
            build::compile_package(&package_dir, verbose).await
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
            inject_message::execute(url, process, ipc, node, bytes).await
        },
        Some(("new", new_matches)) => {
            let new_dir = PathBuf::from(new_matches.get_one::<String>("directory").unwrap());
            let package_name = new_matches.get_one::<String>("package-name").unwrap();
            let publisher = new_matches.get_one::<String>("publisher").unwrap();

            new::execute(new_dir, package_name.clone(), publisher.clone())
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

            run_tests::execute(config_path.to_str().unwrap()).await
        },
        Some(("start-package", start_package_matches)) => {
            let package_dir = PathBuf::from(start_package_matches.get_one::<String>("package-dir").unwrap());
            let url: &String = start_package_matches.get_one("url").unwrap();
            let node: Option<&str> = start_package_matches
                .get_one("node")
                .and_then(|s: &String| Some(s.as_str()));
            start_package::execute(package_dir, url, node).await
        },
        _ => {
            println!("Invalid subcommand. Usage:\n{}", usage);
            Ok(())
        },
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let current_dir = env::current_dir()?.into_os_string();
    let mut app = command!()
        .name("UqDev")
        .version("0.1.0")
        .about("Development tools for Uqbar")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("boot-fake-node")
            .about("Boot a fake node for development")
            .arg(Arg::new("runtime-path")
                .action(ArgAction::Set)
                .long("runtime-path")
                .help("Path to Uqbar core repo or runtime binary (overrides --version)")
            )
            .arg(Arg::new("version")
                .action(ArgAction::Set)
                .short('v')
                .long("version")
                .help("Version of Uqbar binary to use (overridden by --runtime-path)")
                .default_value("0.4.0")
            )
            .arg(Arg::new("node-home")
                .action(ArgAction::Set)
                .short('h')
                .long("home")
                .help("Where to place the home directory for the fake node")
                .default_value("/tmp/uqbar-fake-node")
            )
            .arg(Arg::new("node-port")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("The port to run the fake node on")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("network-router-port")
                .action(ArgAction::Set)
                .long("network-router-port")
                .help("The port to run the network router on")
                .default_value("9001")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("rpc")
                .action(ArgAction::Set)
                .short('r')
                .long("rpc")
                .help("Ethereum RPC endpoint (wss://)")
                .required(false)
            )
            .arg(Arg::new("fake-node-name")
                .action(ArgAction::Set)
                .short('f')
                .long("fake-node-name")
                .help("Name for fake node")
                .default_value("fake.uq")
            )
            .arg(Arg::new("password")
                .action(ArgAction::Set)
                .long("password")
                .help("Password to login")
                .default_value("secret")
            )
        )
        .subcommand(Command::new("build")
            .about("Build an Uqbar process")
            .arg(Arg::new("package-dir")
                .action(ArgAction::Set)
                .help("The package directory to build")
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
            .about("Create an Uqbar template package")
            .arg(Arg::new("directory")
                .action(ArgAction::Set)
                .help("Path to create template directory at")
                .required(true)
            )
            .arg(Arg::new("package-name")
                .action(ArgAction::Set)
                .short('a')
                .long("package")
                .help("Name of the package")
                .required(true)
            )
            .arg(Arg::new("publisher")
                .action(ArgAction::Set)
                .short('u')
                .long("package")
                .help("Name of the publisher")
                .default_value("template.uq")
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
            .arg(Arg::new("package-dir")
                .action(ArgAction::Set)
                .help("The package directory to build")
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

    match execute(usage, matches).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // TODO: add more non-"nerdview" error messages here
            match e.downcast_ref::<reqwest::Error>() {
                None => {},
                Some(e) => {
                    if e.is_connect() {
                        println!("uqdev: error connecting; is Uqbar node running?");
                        return Ok(());
                    }
                },
            }
            Err(e)
        },
    }
}
