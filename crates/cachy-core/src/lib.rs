//! cachy-core: agent engine, tools, providers and persistence for cachycli.

pub mod acp;
pub mod activity;
pub mod agent;
pub mod auth;
pub mod config;
pub mod export;
pub mod hooks;
pub mod inspect;
pub mod jsonschema;
pub mod mcp;
pub mod memory;
pub mod message;
pub mod models;
pub mod paths;
pub mod permissions;
pub mod personas;
pub mod plugins;
pub mod prompt;
pub mod providers;
pub mod session;
pub mod share;
pub mod share_net;
pub mod skills;
pub mod text;
pub mod tools;
pub mod workflows;

/// CLI name.
pub const APP_NAME: &str = "cachycli";
/// Display name used in the TUI.
pub const APP_TITLE: &str = "Cachy";
/// Version compiled into the binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Full user agent string for HTTP requests.
pub fn user_agent() -> String {
    format!("{APP_NAME}/{VERSION}")
}
