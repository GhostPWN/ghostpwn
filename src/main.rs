mod agent;
mod config;
mod models;
mod providers;
mod secrets;
mod skills;
mod tools;
mod ui;

use std::env;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::config::Config;
use crate::secrets::SecretStore;
use crate::tools::ToolRuntime;

const HELP: &str = "\
GhostPWN - autonomous pentest agent TUI

Usage:
  ghostpwn [OPTIONS]

Options:
  -h, --help     Show this help message
  -V, --version  Show version information

Running without options launches the TUI.
";

#[tokio::main]
async fn main() -> Result<()> {
    if handle_cli_args() {
        return Ok(());
    }

    let config = Config::load()?;
    let tools = ToolRuntime::new(config.workspace_root.clone())?;
    let secret_store = SecretStore::new();

    let agent = Arc::new(Mutex::new(Agent::new(
        config.provider,
        config.model,
        config.provider_keys,
        secret_store,
        tools,
    )));
    ui::run_ui(agent).await
}

fn handle_cli_args() -> bool {
    let mut args = env::args().skip(1);
    let Some(arg) = args.next() else {
        return false;
    };

    match arg.as_str() {
        "-h" | "--help" => {
            print!("{HELP}");
            true
        }
        "-V" | "--version" => {
            println!("ghostpwn {}", env!("CARGO_PKG_VERSION"));
            true
        }
        _ => false,
    }
}
