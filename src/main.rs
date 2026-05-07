use std::io;

use explore_ai_agent::cli;
use explore_ai_agent::common::config::AppConfig;

#[tokio::main]
async fn main() -> Result<(), String> {
    let config = AppConfig::load(None)?;
    cli::run_cli_with_io(&config, io::stdin().lock(), io::stdout()).await
}
