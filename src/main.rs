mod agent;
mod config;
mod models;
mod providers;
mod tools;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::config::Config;
use crate::providers::build_provider;
use crate::tools::ToolRuntime;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let provider = build_provider(&config)?;
    let tools = ToolRuntime::new(config.workspace_root.clone())?;

    let agent = Arc::new(Mutex::new(Agent::new(provider, tools)));
    ui::run_ui(agent).await
}
