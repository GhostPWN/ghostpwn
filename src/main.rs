mod agent;
mod config;
mod images;
mod models;
mod providers;
mod secrets;
mod skills;
mod tools;
mod ui;

use std::env;
use std::sync::Arc;

use anyhow::{Result, anyhow};
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
    if handle_cli_args()? {
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

fn handle_cli_args() -> Result<bool> {
    match parse_cli_args(env::args().skip(1))? {
        CliAction::Launch => Ok(false),
        CliAction::Help => {
            print!("{HELP}");
            Ok(true)
        }
        CliAction::Version => {
            println!("ghostpwn {}", env!("CARGO_PKG_VERSION"));
            Ok(true)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    Launch,
    Help,
    Version,
}

fn parse_cli_args(args: impl IntoIterator<Item = String>) -> Result<CliAction> {
    let args = args.into_iter().collect::<Vec<_>>();
    match args.as_slice() {
        [] => Ok(CliAction::Launch),
        [arg] if matches!(arg.as_str(), "-h" | "--help") => Ok(CliAction::Help),
        [arg] if matches!(arg.as_str(), "-V" | "--version") => Ok(CliAction::Version),
        [arg] => Err(anyhow!("unknown argument '{arg}'\n\n{HELP}")),
        [arg, trailing @ ..] => Err(anyhow!(
            "unexpected arguments after '{arg}': {}\n\n{HELP}",
            trailing.join(" ")
        )),
    }
}

#[cfg(test)]
mod cli_tests {
    use super::{CliAction, parse_cli_args};

    #[test]
    fn parses_supported_cli_actions() {
        assert_eq!(parse_cli_args([]).unwrap(), CliAction::Launch);
        assert_eq!(
            parse_cli_args(["--help".to_string()]).unwrap(),
            CliAction::Help
        );
        assert_eq!(
            parse_cli_args(["-V".to_string()]).unwrap(),
            CliAction::Version
        );
    }

    #[test]
    fn rejects_unknown_and_trailing_arguments() {
        let unknown = parse_cli_args(["--verison".to_string()]).unwrap_err();
        assert!(unknown.to_string().contains("unknown argument '--verison'"));

        let trailing = parse_cli_args(["--help".to_string(), "extra".to_string()]).unwrap_err();
        assert!(
            trailing
                .to_string()
                .contains("unexpected arguments after '--help': extra")
        );
    }
}
