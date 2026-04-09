mod agent;
mod config;
mod models;
mod providers;
mod secrets;
mod tools;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::config::Config;
use crate::secrets::SecretStore;
use crate::tools::ToolRuntime;

#[tokio::main]
async fn main() -> Result<()> {
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
