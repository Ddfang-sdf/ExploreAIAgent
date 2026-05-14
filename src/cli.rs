use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::adapter::api_adapter::{ApiAdapter, ApiMode};
use crate::adapter::model::{ModelAdapter, OpenAiChatAdapter, AnthropicMessagesAdapter};
use crate::common::config::AppConfig;
use crate::conversation::manager::ConversationManager;
use crate::orchestrator::orchestrator::Orchestrator;
use crate::tools::registry::ToolRegistry;

#[derive(Clone)]
pub struct CoreModules {
    pub adapter: Arc<ApiAdapter>,
    pub registry: Arc<ToolRegistry>,
    pub conversation_manager: ConversationManager,
    pub orchestrator: Orchestrator,
}

pub fn assemble_core(config: &AppConfig) -> Result<CoreModules, String> {
    let mut adapter = ApiAdapter::from_config(&config.llm);
    let model_adapter: std::sync::Arc<dyn ModelAdapter> = if config.llm.api_protocol == "anthropic" {
        std::sync::Arc::new(AnthropicMessagesAdapter::new("anthropic"))
    } else {
        std::sync::Arc::new(
            OpenAiChatAdapter::new("openai")
                .with_thinking(config.llm.thinking)
                .with_reasoning_split(true)
        )
    };
    adapter.model_adapter = Some(model_adapter.clone());
    let adapter = Arc::new(adapter);

    let workspace = PathBuf::from(&config.workspace.path);
    let registry = Arc::new(ToolRegistry::new(workspace));

    let conversation_manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));

    let mut orchestrator = Orchestrator::from_config(
        adapter.clone(),
        registry.clone(),
        ConversationManager::new(ApiAdapter::new(ApiMode::Chat)),
        &config.exploration,
        &config.deep_explorer,
        &config.fast_explore,
    );
    orchestrator.shell_output_lines = config.tools.shell_max_output_lines;
    orchestrator.shell_output_bytes = config.tools.shell_max_output_bytes;
    orchestrator.model_adapter = Some(model_adapter);

    Ok(CoreModules {
        adapter,
        registry,
        conversation_manager,
        orchestrator,
    })
}

pub async fn run_cli_with_io<R: BufRead, W: Write>(
    config: &AppConfig,
    mut reader: R,
    mut writer: W,
) -> Result<(), String> {
    let mut core = assemble_core(config)?;
    let session_id = "cli-session";

    core.conversation_manager.init_session(session_id);

    let mut line = String::new();

    writeln!(writer, "Explore AI Agent (CLI mode). 输入 /exit 或 exit 退出。ESC 中断回答。")
        .map_err(|e| e.to_string())?;

    loop {
        write!(writer, ">>> ").map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;

        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 { break; } // EOF (pipe closed)
        let input = line.trim().to_string();

        if input.is_empty() { continue; }
        if input == "/exit" || input == "/quit" || input == "exit" || input == "quit" { break; }

        let output = core.conversation_manager.get_context(session_id).ok();
        let previous_messages = output.as_ref().map(|c| c.previous_messages.as_slice()).unwrap_or(&[]);

        let t0 = std::time::Instant::now();
        let _raw_mode = crate::terminal::RawModeGuard::enter()?;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (esc_thread, esc_running) = crate::terminal::spawn_esc_listener(cancel_flag.clone());

        let result = core.orchestrator.run(&input, previous_messages, cancel_flag).await;

        esc_running.store(false, std::sync::atomic::Ordering::SeqCst);
        esc_thread.join().ok();
        drop(_raw_mode);

        match result {
            Ok((answer, round_messages)) => {
                let elapsed = t0.elapsed().as_secs_f64();
                if answer.is_empty() {
                    eprintln!("\r\x1b[K⏹ Interrupted ({:.1}s)", elapsed);
                } else {
                    eprintln!("\r\x1b[K✅ 回答完成 ({:.1}s)", elapsed);
                }
                let summary: String = answer.chars().take(200).collect();
                let _ = core.conversation_manager.save_conversation(session_id, &input, &summary, round_messages);
                if !answer.is_empty() {
                    writeln!(writer, "{}", answer).map_err(|e| e.to_string())?;
                }
            }
            Err(e) => {
                eprintln!("\r\x1b[K❌ 出错");
                writeln!(writer, "[ERROR] {}", e).map_err(|e| e.to_string())?;
            }
        }
    }

    Ok(())
}
