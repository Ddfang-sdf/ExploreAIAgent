use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use crate::adapter::api_adapter::{ApiAdapter, ApiMode};
use crate::common::config::AppConfig;
use crate::context::exploration::ExplorationContextTool;
use crate::conversation::manager::ConversationManager;
use crate::orchestrator::orchestrator::Orchestrator;
use crate::tools::registry::ToolRegistry;

pub struct CoreModules {
    pub adapter: Arc<ApiAdapter>,
    pub registry: Arc<ToolRegistry>,
    pub conversation_manager: ConversationManager,
    pub orchestrator: Orchestrator,
}

pub fn assemble_core(config: &AppConfig) -> Result<CoreModules, String> {
    let adapter = Arc::new(ApiAdapter::from_config(&config.llm));

    let workspace = PathBuf::from(&config.workspace.path);
    let registry = Arc::new(ToolRegistry::new(workspace));

    let conversation_manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));

    let orchestrator = Orchestrator::from_config(
        adapter.clone(),
        registry.clone(),
        ConversationManager::new(ApiAdapter::new(ApiMode::Chat)),
        &config.exploration,
    );

    Ok(CoreModules {
        adapter,
        registry,
        conversation_manager,
        orchestrator,
    })
}

pub async fn run_cli_with_io<R: BufRead, W: Write>(
    config: &AppConfig,
    reader: R,
    mut writer: W,
) -> Result<(), String> {
    let core = assemble_core(config)?;

    let mut ect = ExplorationContextTool::new("cli-session".to_string());
    ect.configure(&config.exploration, &config.context);

    let mut line = String::new();
    let mut reader = reader;

    writeln!(writer, "Explore AI Agent (CLI mode). 输入 /exit 或 exit 退出。")
        .map_err(|e| e.to_string())?;

    loop {
        write!(writer, ">>> ").map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;

        line.clear();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let input = line.trim().to_string();

        if input.is_empty() {
            continue;
        }
        if input == "/exit" || input == "/quit" || input == "exit" || input == "quit" {
            break;
        }

        match core.orchestrator.run(&input, &mut ect).await {
            Ok(answer) => {
                writeln!(writer, "{}", answer).map_err(|e| e.to_string())?;
            }
            Err(e) => {
                writeln!(writer, "[ERROR] {}", e).map_err(|e| e.to_string())?;
            }
        }
    }

    Ok(())
}
