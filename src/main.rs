use clap::{Arg, ArgAction, builder::PossibleValuesParser, command, Command, value_parser};
use std::env;
use std::path::PathBuf;

mod boot_fake_node;
mod build;
mod build_start_package;
mod dev_ui;
mod inject_message;
mod new;
mod remove_package;
mod run_tests;
mod setup;
mod start_package;
mod update;

async fn execute(
    usage: clap::builder::StyledStr,
    matches: Option<(&str, &clap::ArgMatches)>,
) -> anyhow::Result<()> {
    match matches {
        Some(("boot-fake-node", boot_matches)) => {
            let runtime_path = boot_matches
                .get_one::<String>("PATH")
                .and_then(|p| Some(PathBuf::from(p)));
            let version = boot_matches.get_one::<String>("VERSION").unwrap();
            let node_home = PathBuf::from(boot_matches.get_one::<String>("HOME").unwrap());
            let node_port = boot_matches.get_one::<u16>("NODE_PORT").unwrap();
            let network_router_port = boot_matches.get_one::<u16>("NETWORK_ROUTER_PORT").unwrap();
            let rpc = boot_matches.get_one::<String>("RPC_ENDPOINT").and_then(|s| Some(s.as_str()));
            let fake_node_name = boot_matches.get_one::<String>("NODE_NAME").unwrap();
            let password = boot_matches.get_one::<String>("PASSWORD").unwrap();
            let is_persist = boot_matches.get_one::<bool>("PERSIST").unwrap();

            boot_fake_node::execute(
                runtime_path,
                version.clone(),
                node_home,
                *node_port,
                *network_router_port,
                rpc,
                fake_node_name,
                password,
                *is_persist,
                vec![],
            ).await
        },
        Some(("build", build_matches)) => {
            let package_dir = PathBuf::from(build_matches.get_one::<String>("DIR").unwrap());
            let no_ui = build_matches.get_one::<bool>("NO_UI").unwrap();
            let ui_only = build_matches.get_one::<bool>("UI_ONLY").unwrap();
            let verbose = !build_matches.get_one::<bool>("QUIET").unwrap();
            let skip_deps_check = build_matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();

            build::execute(&package_dir, *no_ui, *ui_only, verbose, *skip_deps_check).await
        },
        Some(("build-start-package", build_start_matches)) => {

            let package_dir = PathBuf::from(build_start_matches.get_one::<String>("DIR").unwrap());
            let no_ui = build_start_matches.get_one::<bool>("NO_UI").unwrap();
            let ui_only = build_start_matches.get_one::<bool>("UI_ONLY").unwrap_or(&false);
            let verbose = !build_start_matches.get_one::<bool>("QUIET").unwrap();
            let url: String = match build_start_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = build_start_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                },
            };
            let skip_deps_check = build_start_matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();

            build_start_package::execute(
                &package_dir,
                *no_ui,
                *ui_only,
                verbose,
                &url,
                *skip_deps_check,
            ).await
        },
        Some(("dev-ui", dev_ui_matches)) => {
            let package_dir = PathBuf::from(dev_ui_matches.get_one::<String>("DIR").unwrap());
            let url: String = match dev_ui_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = dev_ui_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                },
            };
            let skip_deps_check = dev_ui_matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();

            dev_ui::execute(&package_dir, &url, *skip_deps_check)
        },
        Some(("inject-message", inject_message_matches)) => {
            let url: String = match inject_message_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = inject_message_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                },
            };
            let process: &String = inject_message_matches.get_one("PROCESS").unwrap();
            let non_block: &bool = inject_message_matches.get_one("NONBLOCK").unwrap();
            let body: &String = inject_message_matches.get_one("BODY_JSON").unwrap();
            let node: Option<&str> = inject_message_matches
                .get_one("NODE_NAME")
                .and_then(|s: &String| Some(s.as_str()));
            let bytes: Option<&str> = inject_message_matches
                .get_one("PATH")
                .and_then(|s: &String| Some(s.as_str()));

            let expects_response =
                if *non_block {
                    None
                } else {
                    Some(15)
                };
            inject_message::execute(&url, process, expects_response, body, node, bytes).await
        },
        Some(("new", new_matches)) => {
            let new_dir = PathBuf::from(new_matches.get_one::<String>("DIR").unwrap());
            let package_name = new_matches.get_one::<String>("PACKAGE")
                .map(|pn| pn.to_string());
            let publisher = new_matches.get_one::<String>("PUBLISHER").unwrap();
            let language: new::Language = new_matches.get_one::<String>("LANGUAGE").unwrap().into();
            let template: new::Template = new_matches.get_one::<String>("TEMPLATE").unwrap().into();
            let ui = new_matches.get_one::<bool>("UI").unwrap_or(&false);

            new::execute(
                new_dir,
                package_name,
                publisher.clone(),
                language.clone(),
                template.clone(),
                *ui,
            )
        },
        Some(("run-tests", run_tests_matches)) => {
            let config_path = match run_tests_matches.get_one::<String>("PATH") {
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
        Some(("remove-package", remove_package_matches)) => {
            let package_name = remove_package_matches.get_one::<String>("PACKAGE")
                .and_then(|s: &String| Some(s.as_str()));
            let publisher = remove_package_matches.get_one::<String>("PUBLISHER")
                .and_then(|s: &String| Some(s.as_str()));
            let package_dir = PathBuf::from(remove_package_matches.get_one::<String>("DIR").unwrap());
            let url: String = match remove_package_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = remove_package_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                },
            };
            remove_package::execute(&package_dir, &url, package_name, publisher).await
        },
        Some(("setup", _setup_matches)) => setup::execute(),
        Some(("start-package", start_package_matches)) => {
            let package_dir = PathBuf::from(start_package_matches.get_one::<String>("DIR").unwrap());
            let url: String = match start_package_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = start_package_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                },
            };
            start_package::execute(&package_dir, &url).await
        },
        Some(("update", update_matches)) => {
            let args = update_matches.get_many::<String>("ARGUMENTS")
                .unwrap_or_default()
                .map(|v| v.to_string())
                .collect::<Vec<_>>();
            let branch = update_matches.get_one::<String>("BRANCH").unwrap();

            update::execute(args, branch)
        },
        _ => {
            println!("Invalid subcommand. Usage:\n{}", usage);
            Ok(())
        },
    }
}

async fn make_app(current_dir: &std::ffi::OsString) -> anyhow::Result<Command> {
    Ok(command!()
        .name("kit")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Development tool\x1b[1mkit\x1b[0m for Kinode")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .disable_version_flag(true)
        .arg(Arg::new("version")
            .short('v')
            .long("version")
            .action(ArgAction::Version)
            .help("Print version")
        )
        .subcommand(Command::new("boot-fake-node")
            .about("Boot a fake node for development")
            .visible_alias("f")
            .disable_help_flag(true)
            .arg(Arg::new("PATH")
                .action(ArgAction::Set)
                .short('r')
                .long("runtime-path")
                .help("Path to Kinode core repo or runtime binary (overrides --version)")
            )
            .arg(Arg::new("VERSION")
                .action(ArgAction::Set)
                .short('v')
                .long("version")
                .help("Version of Kinode binary to use (overridden by --runtime-path)")
                .default_value("latest")
                .value_parser(PossibleValuesParser::new({
                    let mut possible_values = vec!["latest".to_string()];
                    let mut remote_values = boot_fake_node::find_releases_with_asset_if_online(
                        None,
                        None,
                        &boot_fake_node::get_platform_runtime_name()?
                    ).await?;
                    possible_values.append(&mut remote_values);
                    possible_values
                }))
            )
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("The port to run the fake node on")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("HOME")
                .action(ArgAction::Set)
                .short('h')
                .long("home")
                .help("Where to place the home directory for the fake node")
                .default_value("/tmp/kinode-fake-node")
            )
            .arg(Arg::new("NODE_NAME")
                .action(ArgAction::Set)
                .short('f')
                .long("fake-node-name")
                .help("Name for fake node")
                .default_value("fake.os")
            )
            .arg(Arg::new("NETWORK_ROUTER_PORT")
                .action(ArgAction::Set)
                .long("network-router-port")
                .help("The port to run the network router on (or to connect to)")
                .default_value("9001")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("RPC_ENDPOINT")
                .action(ArgAction::Set)
                .long("rpc")
                .help("Ethereum RPC endpoint (wss://)")
                .required(false)
            )
            .arg(Arg::new("PERSIST")
                .action(ArgAction::SetTrue)
                .long("persist")
                .help("If set, do not delete node home after exit")
                .required(false)
            )
            .arg(Arg::new("PASSWORD")
                .action(ArgAction::Set)
                .long("password")
                .help("Password to login")
                .default_value("secret")
            )
            .arg(Arg::new("help")
                .long("help")
                .action(ArgAction::Help)
                .help("Print help")
            )
        )
        .subcommand(Command::new("build")
            .about("Build a Kinode package")
            .visible_alias("b")
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("The package directory to build")
                .default_value(current_dir)
            )
            .arg(Arg::new("NO_UI")
                .action(ArgAction::SetTrue)
                .long("no-ui")
                .help("If set, do NOT build the web UI for the process; no-op if passed with UI_ONLY")
                .required(false)
            )
            .arg(Arg::new("UI_ONLY")
                .action(ArgAction::SetTrue)
                .long("ui-only")
                .help("If set, build ONLY the web UI for the process; no-op if passed with NO_UI")
                .required(false)
            )
            .arg(Arg::new("QUIET")
                .action(ArgAction::SetTrue)
                .short('q')
                .long("quiet")
                .help("If set, do not print build stdout/stderr")
                .required(false)
            )
            .arg(Arg::new("SKIP_DEPS_CHECK")
                .action(ArgAction::SetTrue)
                .short('s')
                .long("skip-deps-check")
                .help("If set, do not check for dependencies")
                .required(false)
            )
        )
        .subcommand(Command::new("build-start-package")
            .about("Build and start a Kinode package")
            .visible_alias("bs")
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("The package directory to build")
                .default_value(current_dir)
            )
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("Node port: for use on localhost (overridden by URL)")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("URL")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .help("Node URL (overrides NODE_PORT)")
                .required(false)
            )
            .arg(Arg::new("NO_UI")
                .action(ArgAction::SetTrue)
                .long("no-ui")
                .help("If set, do NOT build the web UI for the process; no-op if passed with UI_ONLY")
                .required(false)
            )
            .arg(Arg::new("UI_ONLY")
                .action(ArgAction::SetTrue)
                .long("ui-only")
                .help("If set, build ONLY the web UI for the process")
                .required(false)
            )
            .arg(Arg::new("QUIET")
                .action(ArgAction::SetTrue)
                .short('q')
                .long("quiet")
                .help("If set, do not print build stdout/stderr")
                .required(false)
            )
            .arg(Arg::new("SKIP_DEPS_CHECK")
                .action(ArgAction::SetTrue)
                .short('s')
                .long("skip-deps-check")
                .help("If set, do not check for dependencies")
                .required(false)
            )
        )
        .subcommand(Command::new("dev-ui")
            .about("Start the web UI development server with hot reloading (same as `cd ui && npm i && npm start`)")
            .visible_alias("d")
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("The package directory to build (must contain a `ui` directory)")
                .default_value(current_dir)
            )
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("Node port: for use on localhost (overridden by URL)")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("URL")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .help("Node URL (overrides NODE_PORT)")
                .required(false)
            )
            .arg(Arg::new("SKIP_DEPS_CHECK")
                .action(ArgAction::SetTrue)
                .short('s')
                .long("skip-deps-check")
                .help("If set, do not check for dependencies")
                .required(false)
            )
        )
        .subcommand(Command::new("inject-message")
            .about("Inject a message to a running Kinode")
            .visible_alias("i")
            .arg(Arg::new("PROCESS")
                .action(ArgAction::Set)
                .help("PROCESS to send message to")
                .required(true)
            )
            .arg(Arg::new("BODY_JSON")
                .action(ArgAction::Set)
                .help("Body in JSON format")
                .required(true)
            )
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("Node port: for use on localhost (overridden by URL)")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("URL")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .help("Node URL (overrides NODE_PORT)")
                .required(false)
            )
            .arg(Arg::new("NODE_NAME")
                .action(ArgAction::Set)
                .short('n')
                .long("node")
                .help("Node ID (default: our)")
                .required(false)
            )
            .arg(Arg::new("PATH")
                .action(ArgAction::Set)
                .short('b')
                .long("blob")
                .help("Send file at Unix path as bytes blob")
                .required(false)
            )
            .arg(Arg::new("NONBLOCK")
                .action(ArgAction::SetTrue)
                .short('l')
                .long("non-block")
                .help("If set, don't block on the full node response")
            )
        )
        .subcommand(Command::new("new")
            .about("Create a Kinode template package")
            .visible_alias("n")
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("Path to create template directory at")
                .required(true)
            )
            .arg(Arg::new("PACKAGE")
                .action(ArgAction::Set)
                .short('a')
                .long("package")
                .help("Name of the package [default: DIR]")
            )
            .arg(Arg::new("PUBLISHER")
                .action(ArgAction::Set)
                .short('u')
                .long("publisher")
                .help("Name of the publisher")
                .default_value("template.os")
            )
            .arg(Arg::new("LANGUAGE")
                .action(ArgAction::Set)
                .short('l')
                .long("language")
                .help("Programming language of the template")
                .value_parser(["rust", "python", "javascript"])
                .default_value("rust")
            )
            .arg(Arg::new("TEMPLATE")
                .action(ArgAction::Set)
                .short('t')
                .long("template")
                .help("Template to create")
                .value_parser(["chat", "echo", "fibonacci"])
                .default_value("chat")
            )
            .arg(Arg::new("UI")
                .action(ArgAction::SetTrue)
                .long("ui")
                .help("If set, use the template with UI")
                .required(false)
            )
        )
        .subcommand(Command::new("run-tests")
            .about("Run Kinode tests")
            .visible_alias("t")
            .arg(Arg::new("PATH")
                .action(ArgAction::Set)
                .help("Path to tests configuration file")
                .default_value("tests.toml")
            )
        )
        .subcommand(Command::new("remove-package")
            .about("Remove a running package from a node")
            .visible_alias("r")
            .arg(Arg::new("PACKAGE")
                .action(ArgAction::Set)
                .short('a')
                .long("package")
                .help("Name of the package (Overrides DIR)")
                .required(false)
            )
            .arg(Arg::new("PUBLISHER")
                .action(ArgAction::Set)
                .long("publisher")
                .help("Name of the publisher (Overrides DIR)")
                .required(false)
            )
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("The package directory to remove (Overridden by PACKAGE/PUBLISHER)")
                .default_value(current_dir)
            )
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("Node port: for use on localhost (overridden by URL)")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("URL")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .help("Node URL (overrides NODE_PORT)")
                .required(false)
                //.default_value("http://localhost:8080")
            )
        )
        .subcommand(Command::new("setup")
            .about("Fetch & setup kit dependencies")
        )
        .subcommand(Command::new("start-package")
            .about("Start a built Kinode process")
            .visible_alias("s")
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("The package directory to build")
                .default_value(current_dir)
            )
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("Node port: for use on localhost (overridden by URL)")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("URL")
                .action(ArgAction::Set)
                .short('u')
                .long("url")
                .help("Node URL (overrides NODE_PORT)")
                .required(false)
            )
        )
        .subcommand(Command::new("update")
            .about("Fetch the most recent version of kit")
            .arg(Arg::new("ARGUMENTS")
                .action(ArgAction::Append)
                .help("Additional arguments (e.g. `--branch next-release`)")
                .required(false)
            )
            .arg(Arg::new("BRANCH")
                .action(ArgAction::Set)
                .long("branch")
                .help("Branch name (e.g. `next-release`)")
                .default_value("master")
            )
        )
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let current_dir = env::current_dir()?.into_os_string();
    let mut app = make_app(&current_dir).await?;

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
                        println!("kit: error connecting; is Kinode running?");
                        return Ok(());
                    }
                },
            }
            Err(e)
        },
    }
}
