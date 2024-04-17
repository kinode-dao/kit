use clap::{builder::PossibleValuesParser, command, value_parser, Arg, ArgAction, Command};
use std::env;
use std::path::PathBuf;
use std::str::FromStr;

use color_eyre::{eyre::{eyre, Result}, Section};
use fs_err as fs;
use serde::Deserialize;
use tracing::{error, warn, Level};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    filter, fmt, layer::SubscriberExt, prelude::*, util::SubscriberInitExt, EnvFilter,
};

use kit::{
    boot_fake_node,
    build,
    build_start_package,
    dev_ui,
    inject_message,
    new,
    remove_package,
    reset_cache,
    run_tests,
    setup,
    start_package,
    update,
    KIT_LOG_PATH_DEFAULT,
};

const MAX_REMOTE_VALUES: usize = 3;
const GIT_COMMIT_HASH: &str = env!("GIT_COMMIT_SHA");
const GIT_BRANCH_NAME: &str = env!("GIT_BRANCH_NAME");
const KIT_REPO: &str = "kit";
const KIT_MASTER_BRANCH: &str = "master";
const STDOUT_LOG_LEVEL_DEFAULT: Level = Level::INFO;
const STDERR_LOG_LEVEL_DEFAULT: &str = "error";
const FILE_LOG_LEVEL_DEFAULT: &str = "debug";
const RUST_LOG: &str = "RUST_LOG";

#[derive(Debug, Deserialize)]
struct Commit {
    sha: String,
}

async fn get_latest_commit_sha_from_branch(
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<Commit> {
    let bytes = boot_fake_node::get_from_github(owner, repo, &format!("commits/{branch}")).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn init_tracing(log_path: PathBuf) -> tracing_appender::non_blocking::WorkerGuard {
    // Define a fixed log file name with rolling based on size or execution instance.
    let log_parent_path = log_path
        .parent()
        .unwrap();
    let log_file_name = log_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap();
    if !log_parent_path.exists() {
        fs::create_dir_all(log_parent_path).unwrap();
    }
    let file_appender = tracing_appender::rolling::never(log_parent_path, log_file_name);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let level = std::env::var(RUST_LOG)
        .ok()
        .and_then(|l| Level::from_str(&l).ok())
        .unwrap_or_else(|| STDOUT_LOG_LEVEL_DEFAULT);
    let allowed_levels: Vec<Level> = vec![Level::INFO, Level::WARN]
        .into_iter()
        .filter(|&l| l <= level)
        .collect();
    let stdout_filter = filter::filter_fn(move |metadata: &tracing::Metadata<'_>| {
        allowed_levels.iter().any(|l| metadata.level() == l)
    });

    let stderr_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(STDERR_LOG_LEVEL_DEFAULT));
    let file_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(FILE_LOG_LEVEL_DEFAULT))
        .add_directive("hyper=off".parse().unwrap())
        .add_directive("reqwest=off".parse().unwrap());

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .without_time()
                .with_writer(std::io::stdout)
                .with_ansi(true)
                .with_level(false)
                .with_target(false)
                .fmt_fields(fmt::format::PrettyFields::new())
                .with_filter(stdout_filter),
        )
        .with(
            fmt::layer()
                .with_file(true)
                .with_line_number(true)
                .without_time()
                .with_writer(std::io::stderr)
                .with_ansi(true)
                .with_level(true)
                .with_target(false)
                .fmt_fields(fmt::format::PrettyFields::new())
                .with_filter(stderr_filter),
        )
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .json()
                .with_filter(file_filter),
        )
        .with(ErrorLayer::default())
        .init();

    guard
}

async fn execute(
    usage: clap::builder::StyledStr,
    matches: Option<(&str, &clap::ArgMatches)>,
) -> Result<()> {
    match matches {
        Some(("boot-fake-node", boot_matches)) => {
            let runtime_path = boot_matches
                .get_one::<String>("PATH")
                .and_then(|p| Some(PathBuf::from(p)));
            let version = boot_matches.get_one::<String>("VERSION").unwrap();
            let node_home = PathBuf::from(boot_matches.get_one::<String>("HOME").unwrap());
            let node_port = boot_matches.get_one::<u16>("NODE_PORT").unwrap();
            let network_router_port = boot_matches.get_one::<u16>("NETWORK_ROUTER_PORT").unwrap();
            let rpc = boot_matches
                .get_one::<String>("RPC_ENDPOINT")
                .and_then(|s| Some(s.as_str()));
            let fake_node_name = boot_matches.get_one::<String>("NODE_NAME").unwrap();
            let password = boot_matches.get_one::<String>("PASSWORD").unwrap();
            let is_persist = boot_matches.get_one::<bool>("PERSIST").unwrap();
            let release = boot_matches.get_one::<bool>("RELEASE").unwrap();
            let verbosity = boot_matches.get_one::<u8>("VERBOSITY").unwrap();

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
                *release,
                *verbosity,
                vec![],
            )
            .await
        }
        Some(("build", build_matches)) => {
            let package_dir = PathBuf::from(build_matches.get_one::<String>("DIR").unwrap());
            let no_ui = build_matches.get_one::<bool>("NO_UI").unwrap();
            let ui_only = build_matches.get_one::<bool>("UI_ONLY").unwrap();
            let skip_deps_check = build_matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();
            let features = match build_matches.get_one::<String>("FEATURES") {
                Some(f) => f.clone(),
                None => "".into(),
            };

            build::execute(&package_dir, *no_ui, *ui_only, *skip_deps_check, &features).await
        }
        Some(("build-start-package", build_start_matches)) => {
            let package_dir = PathBuf::from(build_start_matches.get_one::<String>("DIR").unwrap());
            let no_ui = build_start_matches.get_one::<bool>("NO_UI").unwrap();
            let ui_only = build_start_matches
                .get_one::<bool>("UI_ONLY")
                .unwrap_or(&false);
            let url: String = match build_start_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = build_start_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                }
            };
            let skip_deps_check = build_start_matches
                .get_one::<bool>("SKIP_DEPS_CHECK")
                .unwrap();
            let features = match build_start_matches.get_one::<String>("FEATURES") {
                Some(f) => f.clone(),
                None => "".into(),
            };

            build_start_package::execute(
                &package_dir,
                *no_ui,
                *ui_only,
                &url,
                *skip_deps_check,
                &features,
            )
            .await
        }
        Some(("dev-ui", dev_ui_matches)) => {
            let package_dir = PathBuf::from(dev_ui_matches.get_one::<String>("DIR").unwrap());
            let url: String = match dev_ui_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = dev_ui_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                }
            };
            let skip_deps_check = dev_ui_matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();
            let release = dev_ui_matches.get_one::<bool>("RELEASE").unwrap();

            dev_ui::execute(&package_dir, &url, *skip_deps_check, *release)
        }
        Some(("inject-message", inject_message_matches)) => {
            let url: String = match inject_message_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = inject_message_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                }
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

            let expects_response = if *non_block { None } else { Some(15) };
            inject_message::execute(&url, process, expects_response, body, node, bytes).await
        }
        Some(("new", new_matches)) => {
            let new_dir = PathBuf::from(new_matches.get_one::<String>("DIR").unwrap());
            let package_name = new_matches
                .get_one::<String>("PACKAGE")
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
        }
        Some(("remove-package", remove_package_matches)) => {
            let package_name = remove_package_matches
                .get_one::<String>("PACKAGE")
                .and_then(|s: &String| Some(s.as_str()));
            let publisher = remove_package_matches
                .get_one::<String>("PUBLISHER")
                .and_then(|s: &String| Some(s.as_str()));
            let package_dir =
                PathBuf::from(remove_package_matches.get_one::<String>("DIR").unwrap());
            let url: String = match remove_package_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = remove_package_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                }
            };
            remove_package::execute(&package_dir, &url, package_name, publisher).await
        }
        Some(("reset-cache", _reset_cache_matches)) => {
            reset_cache::execute()
        }
        Some(("run-tests", run_tests_matches)) => {
            let config_path = match run_tests_matches.get_one::<String>("PATH") {
                Some(path) => PathBuf::from(path),
                None => std::env::current_dir()?.join("tests.toml"),
            };

            if !config_path.exists() {
                let error = format!(
                    "Configuration file not found: {:?}\nUsage:\n{}",
                    config_path, usage,
                );
                return Err(eyre!(error));
            }

            run_tests::execute(config_path.to_str().unwrap()).await
        }
        Some(("setup", _setup_matches)) => setup::execute(),
        Some(("start-package", start_package_matches)) => {
            let package_dir =
                PathBuf::from(start_package_matches.get_one::<String>("DIR").unwrap());
            let url: String = match start_package_matches.get_one::<String>("URL") {
                Some(url) => url.clone(),
                None => {
                    let port = start_package_matches.get_one::<u16>("NODE_PORT").unwrap();
                    format!("http://localhost:{}", port)
                }
            };
            start_package::execute(&package_dir, &url).await
        }
        Some(("update", update_matches)) => {
            let args = update_matches
                .get_many::<String>("ARGUMENTS")
                .unwrap_or_default()
                .map(|v| v.to_string())
                .collect::<Vec<_>>();
            let branch = update_matches.get_one::<String>("BRANCH").unwrap();

            update::execute(args, branch)
        }
        _ => {
            warn!("Invalid subcommand. Usage:\n{}", usage);
            Ok(())
        }
    }
}

async fn make_app(current_dir: &std::ffi::OsString) -> Result<Command> {
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
                    remote_values.truncate(MAX_REMOTE_VALUES);
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
            .arg(Arg::new("RELEASE")
                .action(ArgAction::SetTrue)
                .long("release")
                .help("If set and given --runtime-path, compile release build [default: debug build]")
                .required(false)
            )
            .arg(Arg::new("VERBOSITY")
                .action(ArgAction::Set)
                .long("verbosity")
                .help("Verbosity of node: higher is more verbose")
                .default_value("0")
                .value_parser(value_parser!(u8))
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
            .arg(Arg::new("SKIP_DEPS_CHECK")
                .action(ArgAction::SetTrue)
                .short('s')
                .long("skip-deps-check")
                .help("If set, do not check for dependencies")
                .required(false)
            )
            .arg(Arg::new("FEATURES")
                .action(ArgAction::Set)
                .long("features")
                .help("Pass these comma-delimited feature flags to Rust cargo builds")
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
            .arg(Arg::new("SKIP_DEPS_CHECK")
                .action(ArgAction::SetTrue)
                .short('s')
                .long("skip-deps-check")
                .help("If set, do not check for dependencies")
                .required(false)
            )
            .arg(Arg::new("FEATURES")
                .action(ArgAction::Set)
                .long("features")
                .help("Pass these comma-delimited feature flags to Rust cargo builds")
                .required(false)
            )
        )
        .subcommand(Command::new("dev-ui")
            .about("Start the web UI development server with hot reloading (same as `cd ui && npm i && npm run dev`")
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
            .arg(Arg::new("RELEASE")
                .action(ArgAction::SetTrue)
                .long("release")
                .help("If set, create a production build")
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
                .value_parser(["chat", "echo", "fibonacci", "file_transfer"])
                .default_value("chat")
            )
            .arg(Arg::new("UI")
                .action(ArgAction::SetTrue)
                .long("ui")
                .help("If set, use the template with UI")
                .required(false)
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
        .subcommand(Command::new("reset-cache")
            .about("Reset kit cache (Kinode core binaries, logs, etc.)")
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
async fn main() -> Result<()> {
    let log_path =
        std::env::var("KIT_LOG_PATH").unwrap_or_else(|_| KIT_LOG_PATH_DEFAULT.to_string());
    let log_path = PathBuf::from(log_path);
    let _guard = init_tracing(log_path);
    color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .install()?;
    let current_dir = env::current_dir()?.into_os_string();
    let mut app = make_app(&current_dir).await?;

    let usage = app.render_usage();
    let matches = app.get_matches();
    let matches = matches.subcommand();

    let result = match execute(usage, matches).await {
        Ok(()) => Ok(()),
        Err(mut e) => {
            // TODO: add more non-"nerdview" error messages here
            match e.downcast_ref::<reqwest::Error>() {
                None => {}
                Some(ee) => {
                    if ee.is_connect() {
                        e = e.with_suggestion(|| "is Kinode running?");
                    }
                }
            }
            Err(e)
        }
    };

    if let Some((subcommand, _)) = matches {
        if subcommand != "update" && GIT_BRANCH_NAME == "master" {
            let latest = get_latest_commit_sha_from_branch(
                boot_fake_node::KINODE_OWNER,
                KIT_REPO,
                KIT_MASTER_BRANCH,
            )
            .await?;
            if GIT_COMMIT_HASH != latest.sha {
                warn!("kit is out of date! Run:\n```\nkit update\n```\nto update to the latest version.");
            }
        }
    }

    if let Err(e) = result {
        error!("{:?}", e);
    };
    Ok(())
}
