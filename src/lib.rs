pub mod boot_fake_node;
pub mod build;
pub mod build_start_package;
pub mod chain;
pub mod dev_ui;
pub mod inject_message;
pub mod new;
pub mod remove_package;
pub mod reset_cache;
pub mod run_tests;
pub mod setup;
pub mod start_package;
pub mod update;
pub mod view_api;

pub const KIT_CACHE: &str = "/tmp/kinode-kit-cache";
pub const KIT_LOG_PATH_DEFAULT: &str = "/tmp/kinode-kit-cache/logs/log.log";
