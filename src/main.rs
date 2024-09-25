use clap::{builder::PossibleValuesParser, command, value_parser, Arg, ArgAction, Command};
use std::env;
use std::path::PathBuf;
use std::str::FromStr;

use color_eyre::{
    eyre::{eyre, Result},
    Section,
};
use fs_err as fs;
use serde::Deserialize;
use tracing::{error, warn, Level};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    filter, fmt, layer::SubscriberExt, prelude::*, util::SubscriberInitExt, EnvFilter,
};

use kit::{
    build, build_start_package, dev_ui, inject_message, new, publish, remove_package, reset_cache,
    setup, start_package, update, KIT_LOG_PATH_DEFAULT,
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

fn parse_u128_with_underscores(s: &str) -> Result<u128, &'static str> {
    let clean_string = s.replace('_', "");
    clean_string
        .parse::<u128>()
        .map_err(|_| "Invalid number format")
}

fn init_tracing(log_path: PathBuf) -> tracing_appender::non_blocking::WorkerGuard {
    // Define a fixed log file name with rolling based on size or execution instance.
    let log_parent_path = log_path.parent().unwrap();
    let log_file_name = log_path.file_name().and_then(|f| f.to_str()).unwrap();
    if !log_parent_path.exists() {
        fs::create_dir_all(log_parent_path).unwrap();
    }
    let file_appender = tracing_appender::rolling::never(log_parent_path, log_file_name);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let level = std::env::var(RUST_LOG)
        .ok()
        .and_then(|l| Level::from_str(&l).ok())
        .unwrap_or_else(|| STDOUT_LOG_LEVEL_DEFAULT);
    let allowed_levels: std::collections::HashSet<Level> = vec![Level::INFO, Level::WARN]
        .into_iter()
        .filter(|&l| l <= level)
        .collect();
    let stdout_filter = filter::filter_fn(move |metadata: &tracing::Metadata<'_>| {
        allowed_levels.contains(metadata.level())
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
        Some(("build", matches)) => {
            let package_dir = PathBuf::from(matches.get_one::<String>("DIR").unwrap());
            let no_ui = matches.get_one::<bool>("NO_UI").unwrap();
            let ui_only = matches.get_one::<bool>("UI_ONLY").unwrap();
            let skip_deps_check = matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();
            let features = match matches.get_one::<String>("FEATURES") {
                Some(f) => f.clone(),
                None => "".into(),
            };
            let url = matches
                .get_one::<u16>("NODE_PORT")
                .map(|p| format!("http://localhost:{p}"));
            let download_from = matches
                .get_one::<String>("NODE")
                .and_then(|s: &String| Some(s.as_str()));
            let default_world = matches.get_one::<String>("WORLD");
            let local_dependencies: Vec<PathBuf> = matches
                .get_many::<String>("DEPENDENCY_PACKAGE_PATH")
                .unwrap_or_default()
                .map(|s| PathBuf::from(s))
                .collect();
            let add_paths_to_api: Vec<PathBuf> = matches
                .get_many::<String>("PATH")
                .unwrap_or_default()
                .map(|s| PathBuf::from(s))
                .collect();
            let force = matches.get_one::<bool>("FORCE").unwrap();
            let verbose = matches.get_one::<bool>("VERBOSE").unwrap();

            build::execute(
                &package_dir,
                *no_ui,
                *ui_only,
                *skip_deps_check,
                &features,
                url,
                download_from,
                default_world.map(|w| w.as_str()),
                local_dependencies,
                add_paths_to_api,
                *force,
                *verbose,
                false,
            )
            .await
        }
        Some(("build-start-package", matches)) => {
            let package_dir = PathBuf::from(matches.get_one::<String>("DIR").unwrap());
            let no_ui = matches.get_one::<bool>("NO_UI").unwrap();
            let ui_only = matches.get_one::<bool>("UI_ONLY").unwrap_or(&false);
            let url = format!(
                "http://localhost:{}",
                matches.get_one::<u16>("NODE_PORT").unwrap(),
            );
            let skip_deps_check = matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();
            let features = match matches.get_one::<String>("FEATURES") {
                Some(f) => f.clone(),
                None => "".into(),
            };
            let download_from = matches
                .get_one::<String>("NODE")
                .and_then(|s: &String| Some(s.as_str()));
            let default_world = matches.get_one::<String>("WORLD");
            let local_dependencies: Vec<PathBuf> = matches
                .get_many::<String>("DEPENDENCY_PACKAGE_PATH")
                .unwrap_or_default()
                .map(|s| PathBuf::from(s))
                .collect();
            let add_paths_to_api: Vec<PathBuf> = matches
                .get_many::<String>("PATH")
                .unwrap_or_default()
                .map(|s| PathBuf::from(s))
                .collect();
            let force = matches.get_one::<bool>("FORCE").unwrap();
            let verbose = matches.get_one::<bool>("VERBOSE").unwrap();

            build_start_package::execute(
                &package_dir,
                *no_ui,
                *ui_only,
                &url,
                *skip_deps_check,
                &features,
                download_from,
                default_world.map(|w| w.as_str()),
                local_dependencies,
                add_paths_to_api,
                *force,
                *verbose,
            )
            .await
        }
        Some(("dev-ui", matches)) => {
            let package_dir = PathBuf::from(matches.get_one::<String>("DIR").unwrap());
            let url = format!(
                "http://localhost:{}",
                matches.get_one::<u16>("NODE_PORT").unwrap(),
            );
            let skip_deps_check = matches.get_one::<bool>("SKIP_DEPS_CHECK").unwrap();
            let release = matches.get_one::<bool>("RELEASE").unwrap();

            dev_ui::execute(&package_dir, &url, *skip_deps_check, *release)
        }
        Some(("inject-message", matches)) => {
            let url = format!(
                "http://localhost:{}",
                matches.get_one::<u16>("NODE_PORT").unwrap(),
            );
            let process: &String = matches.get_one("PROCESS").unwrap();
            let non_block: &bool = matches.get_one("NONBLOCK").unwrap();
            let body: &String = matches.get_one("BODY_JSON").unwrap();
            let node: Option<&str> = matches
                .get_one("NODE_NAME")
                .and_then(|s: &String| Some(s.as_str()));
            let bytes: Option<&str> = matches
                .get_one("PATH")
                .and_then(|s: &String| Some(s.as_str()));

            let expects_response = if *non_block { None } else { Some(15) };
            inject_message::execute(&url, process, expects_response, body, node, bytes).await
        }
        Some(("new", matches)) => {
            let new_dir = PathBuf::from(matches.get_one::<String>("DIR").unwrap());
            let package_name = matches
                .get_one::<String>("PACKAGE")
                .map(|pn| pn.to_string());
            let publisher = matches.get_one::<String>("PUBLISHER").unwrap();
            let language: new::Language = matches.get_one::<String>("LANGUAGE").unwrap().into();
            let template: new::Template = matches.get_one::<String>("TEMPLATE").unwrap().into();
            let ui = matches.get_one::<bool>("UI").unwrap_or(&false);

            new::execute(
                new_dir,
                package_name,
                publisher.clone(),
                language.clone(),
                template.clone(),
                *ui,
            )
        }
        Some(("publish", matches)) => {
            let package_dir = PathBuf::from(matches.get_one::<String>("DIR").unwrap());
            let metadata_uri = matches.get_one::<String>("URI").unwrap();
            let keystore_path = matches
                .get_one::<String>("PATH")
                .and_then(|kp| Some(PathBuf::from(kp)));
            let ledger = matches.get_one::<bool>("LEDGER").unwrap();
            let trezor = matches.get_one::<bool>("TREZOR").unwrap();
            let rpc_uri = matches.get_one::<String>("RPC_URI").unwrap();
            let real = matches.get_one::<bool>("REAL").unwrap();
            let unpublish = matches.get_one::<bool>("UNPUBLISH").unwrap();
            let gas_limit = matches.get_one::<u128>("GAS_LIMIT").unwrap();
            let max_priority_fee = matches
                .get_one::<u128>("MAX_PRIORITY_FEE_PER_GAS")
                .and_then(|mpf| Some(mpf.clone()));
            let max_fee_per_gas = matches
                .get_one::<u128>("MAX_FEE_PER_GAS")
                .and_then(|mfpg| Some(mfpg.clone()));

            publish::execute(
                &package_dir,
                metadata_uri,
                keystore_path,
                ledger,
                trezor,
                rpc_uri,
                real,
                unpublish,
                *gas_limit,
                max_priority_fee,
                max_fee_per_gas,
            )
            .await
        }
        Some(("remove-package", matches)) => {
            let package_name = matches
                .get_one::<String>("PACKAGE")
                .and_then(|s: &String| Some(s.as_str()));
            let publisher = matches
                .get_one::<String>("PUBLISHER")
                .and_then(|s: &String| Some(s.as_str()));
            let package_dir = PathBuf::from(matches.get_one::<String>("DIR").unwrap());
            let url = format!(
                "http://localhost:{}",
                matches.get_one::<u16>("NODE_PORT").unwrap(),
            );
            remove_package::execute(&package_dir, &url, package_name, publisher).await
        }
        Some(("reset-cache", _matches)) => reset_cache::execute(),
        Some(("setup", matches)) => {
            let verbose = matches.get_one::<bool>("VERBOSE").unwrap();

            setup::execute(*verbose)
        }
        Some(("start-package", matches)) => {
            let package_dir = PathBuf::from(matches.get_one::<String>("DIR").unwrap());
            let url = format!(
                "http://localhost:{}",
                matches.get_one::<u16>("NODE_PORT").unwrap(),
            );
            start_package::execute(&package_dir, &url).await
        }
        Some(("update", matches)) => {
            let args = matches
                .get_many::<String>("ARGUMENTS")
                .unwrap_or_default()
                .map(|v| v.to_string())
                .collect::<Vec<_>>();
            let branch = matches.get_one::<String>("BRANCH").unwrap();

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
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("localhost node port; for remote see https://book.kinode.org/hosted-nodes.html#using-kit-with-your-hosted-node")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("NODE")
                .action(ArgAction::Set)
                .short('d')
                .long("download-from")
                .help("Download API from this node if not found")
                .required(false)
            )
            .arg(Arg::new("WORLD")
                .action(ArgAction::Set)
                .short('w')
                .long("world")
                .help("Fallback WIT world name")
            )
            .arg(Arg::new("DEPENDENCY_PACKAGE_PATH")
                .action(ArgAction::Append)
                .short('l')
                .long("local-dependency")
                .help("Path to local dependency package (can specify multiple times)")
            )
            .arg(Arg::new("PATH")
                .action(ArgAction::Append)
                .short('a')
                .long("add-to-api")
                .help("Path to file to add to api.zip (can specify multiple times)")
            )
            .arg(Arg::new("FORCE")
                .action(ArgAction::SetTrue)
                .short('f')
                .long("force")
                .help("Force a rebuild")
                .required(false)
            )
            .arg(Arg::new("VERBOSE")
                .action(ArgAction::SetTrue)
                .short('v')
                .long("verbose")
                .help("If set, output stdout and stderr")
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
                .help("localhost node port; for remote see https://book.kinode.org/hosted-nodes.html#using-kit-with-your-hosted-node")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("NODE")
                .action(ArgAction::Set)
                .short('d')
                .long("download-from")
                .help("Download API from this node if not found")
                .required(false)
            )
            .arg(Arg::new("WORLD")
                .action(ArgAction::Set)
                .short('w')
                .long("world")
                .help("Fallback WIT world name")
                .required(false)
            )
            .arg(Arg::new("DEPENDENCY_PACKAGE_PATH")
                .action(ArgAction::Append)
                .short('l')
                .long("local-dependency")
                .help("Path to local dependency package (can specify multiple times)")
            )
            .arg(Arg::new("PATH")
                .action(ArgAction::Append)
                .short('a')
                .long("add-to-api")
                .help("Path to file to add to api.zip (can specify multiple times)")
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
            .arg(Arg::new("FORCE")
                .action(ArgAction::SetTrue)
                .short('f')
                .long("force")
                .help("Force a rebuild")
                .required(false)
            )
            .arg(Arg::new("VERBOSE")
                .action(ArgAction::SetTrue)
                .short('v')
                .long("verbose")
                .help("If set, output stdout and stderr")
                .required(false)
            )
        )
        .subcommand(Command::new("dev-ui")
            .about("Start the web UI development server with hot reloading (same as `cd ui && npm i && npm run dev`)")
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
                .help("localhost node port; for remote see https://book.kinode.org/hosted-nodes.html#using-kit-with-your-hosted-node")
                .default_value("8080")
                .value_parser(value_parser!(u16))
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
                .help("localhost node port; for remote see https://book.kinode.org/hosted-nodes.html#using-kit-with-your-hosted-node")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
            .arg(Arg::new("NODE_NAME")
                .action(ArgAction::Set)
                .short('n')
                .long("node")
                .help("Node ID [default: our]")
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
                .value_parser(["blank", "chat", "echo", "fibonacci", "file_transfer"])
                .default_value("chat")
            )
            .arg(Arg::new("UI")
                .action(ArgAction::SetTrue)
                .long("ui")
                .help("If set, use the template with UI")
                .required(false)
            )
        )
        .subcommand(Command::new("publish")
            .about("Publish or update a package")
            .visible_alias("p")
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("The package directory to publish")
                .default_value(current_dir)
            )
            .arg(Arg::new("PATH")
                .action(ArgAction::Set)
                .short('k')
                .long("keystore-path")
                .help("Path to private key keystore (choose 1 of `k`, `l`, `t`)") // TODO: add link to docs?
                .required(false)
            )
            .arg(Arg::new("LEDGER")
                .action(ArgAction::SetTrue)
                .short('l')
                .long("ledger")
                .help("Use Ledger private key (choose 1 of `k`, `l`, `t`)")
                .required(false)
            )
            .arg(Arg::new("TREZOR")
                .action(ArgAction::SetTrue)
                .short('t')
                .long("trezor")
                .help("Use Trezor private key (choose 1 of `k`, `l`, `t`)")
                .required(false)
            )
            .arg(Arg::new("URI")
                .action(ArgAction::Set)
                .short('u')
                .long("metadata-uri")
                .help("URI where metadata lives")
                .required(true)
            )
            .arg(Arg::new("RPC_URI")
                .action(ArgAction::Set)
                .short('r')
                .long("rpc")
                .help("Ethereum Optimism mainnet RPC endpoint (wss://)")
                .required(true)
            )
            .arg(Arg::new("REAL")
                .action(ArgAction::SetTrue)
                .short('e')
                .long("real")
                .help("If set, deploy to real network [default: fake node]")
                .required(false)
            )
            .arg(Arg::new("UNPUBLISH")
                .action(ArgAction::SetTrue)
                .long("unpublish")
                .help("If set, unpublish existing published package [default: publish a package]")
            )
            .arg(Arg::new("GAS_LIMIT")
                .action(ArgAction::Set)
                .short('g')
                .long("gas-limit")
                .help("The ETH transaction gas limit")
                .default_value("1_000_000")
                .value_parser(clap::builder::ValueParser::new(parse_u128_with_underscores))
                .required(false)
            )
            .arg(Arg::new("MAX_PRIORITY_FEE_PER_GAS")
                .action(ArgAction::Set)
                .short('p')
                .long("priority-fee")
                .help("The ETH transaction max priority fee per gas [default: estimated from network conditions]")
                .value_parser(clap::builder::ValueParser::new(parse_u128_with_underscores))
                .required(false)
            )
            .arg(Arg::new("MAX_FEE_PER_GAS")
                .action(ArgAction::Set)
                .short('f')
                .long("fee-per-gas")
                .help("The ETH transaction max fee per gas [default: estimated from network conditions]")
                .value_parser(clap::builder::ValueParser::new(parse_u128_with_underscores))
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
                .short('u')
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
                .help("localhost node port; for remote see https://book.kinode.org/hosted-nodes.html#using-kit-with-your-hosted-node")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
        )
        .subcommand(Command::new("reset-cache")
            .about("Reset kit cache (Kinode core binaries, logs, etc.)")
        )
        .subcommand(Command::new("setup")
            .about("Fetch & setup kit dependencies")
            .arg(Arg::new("VERBOSE")
                .action(ArgAction::SetTrue)
                .short('v')
                .long("verbose")
                .help("If set, output stdout and stderr")
                .required(false)
            )
        )
        .subcommand(Command::new("start-package")
            .about("Start a built Kinode package")
            .visible_alias("s")
            .arg(Arg::new("DIR")
                .action(ArgAction::Set)
                .help("The package directory to start")
                .default_value(current_dir)
            )
            .arg(Arg::new("NODE_PORT")
                .action(ArgAction::Set)
                .short('p')
                .long("port")
                .help("localhost node port; for remote see https://book.kinode.org/hosted-nodes.html#using-kit-with-your-hosted-node")
                .default_value("8080")
                .value_parser(value_parser!(u16))
            )
        )
        .subcommand(Command::new("update")
            .about("Fetch the most recent version of kit")
            .arg(Arg::new("ARGUMENTS")
                .action(ArgAction::Append)
                .help("Additional arguments to `cargo install` (e.g. `--version <VERSION>`)")
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
    let current_dir = env::current_dir()
        .with_suggestion(|| "Could not fetch CWD. Does CWD exist?")?
        .into_os_string();
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

    if let Err(e) = result {
        error!("{:?}", e);
        std::process::exit(1);
    };
    Ok(())
}
