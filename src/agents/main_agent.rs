use std::sync::Arc;
use tokio::sync::mpsc;
use crate::adapter::api_adapter::{ApiAdapter, LlmStructuredClient, LlmToolClient};
use crate::adapter::model::ModelAdapter;
use crate::adapter::types::ToolDefinition;
use crate::context::exploration::ExplorationContextTool;
use crate::agents::conversation_compactor::ConversationCompactor;

#[derive(Debug, Clone)]
pub enum SseEvent { Thinking(String), Answer(String), Done }
pub static SSE_TX: std::sync::Mutex<Option<mpsc::UnboundedSender<SseEvent>>> = std::sync::Mutex::new(None);
pub fn sse_enable() -> mpsc::UnboundedReceiver<SseEvent> { let (tx, rx) = mpsc::unbounded_channel(); *SSE_TX.lock().unwrap() = Some(tx); rx }
pub fn sse_disable() { *SSE_TX.lock().unwrap() = None; }
pub fn sse_send(e: SseEvent) { if let Some(tx) = SSE_TX.lock().unwrap().as_ref() { let _ = tx.send(e); } }

// ============================================================================
// Trait definitions for dependency injection
// ============================================================================

#[async_trait::async_trait]
pub trait FastExploreExecutor: Send + Sync {
    async fn execute(&self, keywords: &[String]) -> Result<serde_json::Value, String>;
}

#[async_trait::async_trait]
pub trait DeepExploreExecutor: Send + Sync {
    async fn execute(
        &self,
        question: &str,
        current_summary: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String>;
}

#[async_trait::async_trait]
pub trait ShellExecutor: Send + Sync {
    async fn execute(&self, command: &str) -> Result<serde_json::Value, String>;
}

/// Generic tool dispatcher — MainAgent calls this for every tool the LLM invokes.
/// The orchestrator wires shell / FE / DE / base tools behind this single trait.
#[async_trait::async_trait]
pub trait ToolDispatcher: Send + Sync {
    async fn dispatch(&self, tool_name: &str, arguments: &serde_json::Value) -> Result<serde_json::Value, String>;
}

pub struct MainAgent;

impl MainAgent {
    pub fn new() -> Self {
        MainAgent
    }

    /// Decision loop — native function-calling via ModelAdapter, tool dispatch via ToolDispatcher.
    pub async fn run(
        &self,
        question: &str,
        conversation_context: &str,
        tools: Vec<ToolDefinition>,
        dispatcher: &dyn ToolDispatcher,
        client: Arc<ApiAdapter>,
        model_adapter: &dyn ModelAdapter,
        _exploration_context: Arc<ExplorationContextTool>,
        shell_only_mode: bool,
        compact_token_threshold: Option<usize>,
    ) -> Result<String, String> {
        const DEFAULT_COMPACT_THRESHOLD: usize = 8000;
        let compact_usable: Option<usize> = if shell_only_mode {
            Some(compact_token_threshold.unwrap_or(DEFAULT_COMPACT_THRESHOLD))
        } else {
            None
        };
        let mut shell_call_count: usize = 0;
        let mut last_tool_key: Option<String> = None;
        let mut catted_files: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut recent_commands: Vec<String> = Vec::new();
        let mut same_tool_streak: usize = 0;
        const DOOM_LOOP_THRESHOLD: usize = 2;
        const FALLBACK_COMPACT_ROUNDS: usize = 10;

        let system_prompt = Self::assemble_prompt();
        let user_content = if conversation_context.is_empty() {
            question.to_string()
        } else {
            format!("{}\n{}", conversation_context, question)
        };

        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": user_content}),
        ];

        let tools_json = model_adapter.format_tools(&tools);

        // OpenCode-style session retry: no hard cap, 2s base, double each attempt
        let mut llm_error_count: usize = 0;
        const SESSION_BASE_DELAY_MS: u64 = 2000;
        const SESSION_MAX_DELAY_TIMEOUT_MS: u64 = 30_000;
        const SESSION_MAX_DELAY_API_ERROR_MS: u64 = 120_000;

        loop {
            let response = match client
                .invoke_llm_streaming(&messages, &tools_json, None, |text| {
                    eprint!("\x1b[2m{}\x1b[0m", text);
                })
                .await
            {
                Ok((_raw, r)) => {
                    eprintln!(); // separate thinking from tool output
                    r
                }
                Err(e) => {
                    let lower = e.to_lowercase();
                    let is_non_retryable = lower.contains("llm api error (400)")
                        || lower.contains("status code 400")
                        || lower.contains("llm api error (401)")
                        || lower.contains("llm api error (403)")
                        || lower.contains("llm api error (404)")
                        || lower.contains("authentication")
                        || lower.contains("content policy")
                        || lower.contains("invalid request")
                        || lower.contains("bad request")
                        || lower.contains("chat content is empty");
                    if is_non_retryable {
                        return Err(format!("Non-retryable LLM error: {}", e));
                    }

                    llm_error_count += 1;
                    let is_timeout = lower.contains("http timeout");
                    let max_delay_ms = if is_timeout { SESSION_MAX_DELAY_TIMEOUT_MS } else { SESSION_MAX_DELAY_API_ERROR_MS };
                    #[allow(clippy::cast_precision_loss)]
                    let delay_ms = ((SESSION_BASE_DELAY_MS as f64) * 2f64.powi(llm_error_count as i32 - 1)) as u64;
                    let delay = std::time::Duration::from_millis(delay_ms.min(max_delay_ms));

                    eprintln!("\r\x1b[K  \x1b[2m❌ LLM call failed (attempt {}): {}\x1b[0m", llm_error_count, e);
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": format!("LLM API 调用失败(429/超时): {}。请稍等后重试，或基于已有信息直接回答。", e),
                    }));
                    if shell_only_mode && messages.len() > 4 {
                        compact_conversation(&mut messages, &mut recent_commands, client.as_ref() as &dyn LlmToolClient).await;
                    }
                    eprintln!("\r\x1b[K  \x1b[2m⏳ LLM error backoff {}ms (attempt {})...\x1b[0m", delay_ms.min(max_delay_ms), llm_error_count);
                    tokio::time::sleep(delay).await;
                    continue;
                }
            };

            // If the LLM returned text without tool calls → final answer
            if let Some(text) = &response.text {
                if response.tool_calls.is_empty() && !text.is_empty() {
                    return Ok(text.clone());
                }
            }
            // If the LLM returned nothing useful → retry with guidance
            if response.tool_calls.is_empty() {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": "你的回复为空。请使用工具探索代码库，或基于已有信息直接回答用户问题。",
                }));
                continue;
            }

            // --- Tool calls: append assistant message, dispatch each tool, append results ---
            // Build assistant message from raw response (preserves model-specific fields)
            // We need the raw response for this, but call_llm_with_tools returns UnifiedResponse.
            // The raw response is not available here. For now, build a minimal assistant message.
            // TODO: expose raw response from client to properly build assistant message.
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": response.text.clone().unwrap_or_default(),
                "tool_calls": response.tool_calls.iter().map(|tc| {
                    serde_json::json!({
                        "id": tc.id.clone().unwrap_or_default(),
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                        }
                    })
                }).collect::<Vec<_>>(),
            }));

            for tc in &response.tool_calls {
                let tool_name = &tc.name;
                eprintln!("\r\x1b[K  \x1b[2m⬩ {}\x1b[0m", tool_name);
                sse_send(SseEvent::Thinking(tool_name.clone()));

                // Doom-loop detection
                let tool_key = format!("{}|{}", tool_name, serde_json::to_string(&tc.arguments).unwrap_or_default());
                if Some(tool_key.as_str()) == last_tool_key.as_deref() {
                    same_tool_streak += 1;
                } else {
                    same_tool_streak = 1;
                    last_tool_key = Some(tool_key);
                }
                if same_tool_streak >= DOOM_LOOP_THRESHOLD {
                    eprintln!("\r\x1b[K  \x1b[2m⟳ doom-loop warning ({}× same call)\x1b[0m", same_tool_streak);
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": format!(
                            "注意：你已连续 {} 次调用相同工具相同参数。如果确实需要重复，请说明理由；否则请检查已有结果，尝试不同方向。",
                            same_tool_streak
                        ),
                    }));
                    same_tool_streak = 0;
                }

                // Cat-file dedup for execute_shell
                if tool_name == "execute_shell" {
                    if let Some(cmd) = tc.arguments.get("command").and_then(|c| c.as_str()) {
                        let cmd_trimmed = cmd.trim();
                        let cat_target = if cmd_trimmed.starts_with("cat ") || cmd_trimmed.starts_with("cat\t") {
                            cmd_trimmed[3..].trim().split(|c: char| c.is_whitespace() || c == '|' || c == ';').next().unwrap_or("").to_string()
                        } else {
                            String::new()
                        };
                        if !cat_target.is_empty() && catted_files.contains(&cat_target) {
                            eprintln!("\r\x1b[K  \x1b[2m⛔ repeat cat blocked: {}\x1b[0m", cat_target);
                            messages.push(serde_json::json!({
                                "role": "user",
                                "content": format!("你已读取过 {}。如需更多内容，请用 sed -n '行范围p' 读取特定行；或用 grep -n 搜索关键词定位后再读。", cat_target),
                            }));
                        }
                        if !cat_target.is_empty() {
                            catted_files.insert(cat_target);
                        }
                    }
                }

                // Dispatch the tool
                match dispatcher.dispatch(tool_name, &tc.arguments).await {
                    Ok(result) => {
                        let result_str = serde_json::to_string(&result).unwrap_or_default();
                        let preview: String = result_str.chars().take(60).collect();
                        let truncated = result.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
                        let display = if tool_name == "execute_shell" {
                            let cmd = tc.arguments.get("command").and_then(|c| c.as_str()).unwrap_or("");
                            format!("{} {}", tool_name, cmd.chars().take(50).collect::<String>())
                        } else {
                            tool_name.to_string()
                        };
                        eprintln!("\r\x1b[K  \x1b[2m⚡ ok: {} → {}\x1b[0m", display, preview);

                        let mut content = result_str;
                        if truncated {
                            content.push_str("\n\n[输出已截断。此文件可能很大，请改用 grep 搜索关键词定位目标行]");
                        }
                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tc.id.clone().unwrap_or_default(),
                            "content": content,
                        }));

                        if tool_name == "execute_shell" {
                            if let Some(cmd) = tc.arguments.get("command").and_then(|c| c.as_str()) {
                                recent_commands.push(cmd.to_string());
                            }
                            shell_call_count += 1;
                        }
                    }
                    Err(e) => {
                        eprintln!("\r\x1b[K  \x1b[2m⚠ fail: {} | tool: {}\x1b[0m", e, tool_name);
                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tc.id.clone().unwrap_or_default(),
                            "content": format!("执行失败: {}", e),
                        }));
                    }
                }

                // Shell-only mode: conversation-level compaction
                if shell_only_mode && shell_call_count > 0 {
                    let total_tokens: usize = messages.iter()
                        .map(|m| serde_json::to_string(m).unwrap_or_default().len() / 4)
                        .sum();
                    let should_compact = match compact_usable {
                        Some(usable) => total_tokens >= usable,
                        None => shell_call_count >= FALLBACK_COMPACT_ROUNDS,
                    };
                    if should_compact && messages.len() > 3 {
                        compact_conversation(&mut messages, &mut recent_commands, client.as_ref() as &dyn LlmToolClient).await;
                    }
                }
            }
        }
    }

    pub fn assemble_prompt() -> String {
        String::from(
            "你是探索者（Explore AI Agent），一个专业的代码库探索助手。\n\
             你的工作方式是：理解用户问题，调用可用工具获取代码库信息，基于结果回答用户。\n\
             \n\
             ## 回复风格\n\
             - 简洁直接，问什么答什么。1-3 句话或简短段落即可，不要长篇大论。\n\
             - 不要在回答前后加铺垫或总结（如\"以下是结果...\"、\"综上所述...\"），直接给答案。\n\
             - 只在用户明确要求时才使用 emoji。\n\
             - 引用代码位置时使用 file_path:line_number 格式。\n\
             \n\
             ## 工具使用策略\n\
             - 拿到有价值的工具结果后，基于结果推进下一步，不要反复搜索相同内容。\n\
             - 探索代码库时优先用精确的 search_content/search_files 定位，而非 read_file 整个文件再看。\n\
             \n\
             ## 规则\n\
             1. 任何关于代码库的问题，必须先探索再回答。严禁在未探索的情况下猜测或说\"信息不足\"。\n\
             2. 纯问候（\"你好\"、\"谢谢\"）或追问刚探索过的话题可不调工具直接回答。\n\
             3. 严禁编造代码细节。若探索后仍证据不足，如实告知。\n\
             4. 系统检测到连续 2 次相同工具调用时会提醒你检查是否陷入循环。",
        )
    }

    pub fn shell_info() -> String {
        if cfg!(target_os = "windows") {
            if has_usable_bash() { "bash (Windows)".to_string() } else { "cmd.exe (Windows)".to_string() }
        } else {
            "bash (Unix)".to_string()
        }
    }

    pub fn shell_commands() -> String {
        if Self::shell_info().starts_with("cmd") {
            "type dir findstr".to_string()
        } else {
            "cat head tail less grep egrep fgrep find ls tree wc sort uniq cut tr awk sed file stat echo".to_string()
        }
    }

    pub fn shell_notes() -> String {
        let info = Self::shell_info();
        if info.starts_with("cmd") {
            "## Shell 注意：cmd.exe\n\
             - 不支持 && 链式调用，用 & 代替（但 & 不保证前序成功）\n\
             - 变量用 %VAR% 格式\n\
             - 路径有空格时必须用双引号包裹\n\
             - 命令名：type（读文件）、dir（列目录）、findstr（搜索文本）".to_string()
        } else if info.starts_with("bash") {
            "## Shell 注意：Git Bash (Windows)\n\
             - grep 不完全支持 POSIX \\| OR 语法。多词搜索请用 grep -E '(词A|词B)' 而不是 grep '词A\\|词B'\n\
             - 路径有空格时用双引号包裹\n\
             - && 链式调用可用，; 仅在不关心前序失败时使用\n\
             - 用 2>/dev/null 抑制 stderr；用 >/dev/null 抑制 stdout（仅此两种重定向可用）\n\
             - 用 working_dir 参数切换目录，不要用 cd".to_string()
        } else {
            "## Shell 注意：bash (Unix)\n\
             - grep 使用 POSIX BRE 语法，\\| 表示 OR。多词搜索可用 grep -E '(词A|词B)'\n\
             - 路径有空格时用双引号包裹\n\
             - && 链式调用可用；; 在不关心前序失败时使用\n\
             - 用 working_dir 参数切换目录，不要用 cd".to_string()
        }
    }
}

/// Shared compaction helper — used by both the error path and the normal path.
async fn compact_conversation(
    messages: &mut Vec<serde_json::Value>,
    recent_commands: &mut Vec<String>,
    client: &dyn LlmToolClient,
) {
    if messages.len() <= 3 {
        return;
    }
    eprintln!("\r\x1b[K  \x1b[2m🗜️ 对话压缩中...\x1b[0m");
    let keep_recent = 3usize.min(messages.len().saturating_sub(2));
    let mut split_at = messages.len().saturating_sub(keep_recent);
    // Don't split in the middle of an assistant/tool pair.
    // If the first recent message is a tool result, its assistant is in older → push split back.
    while split_at > 2 && messages[split_at].get("role").and_then(|r| r.as_str()) == Some("tool") {
        split_at -= 1;
    }
    // If the last older message is an assistant with tool_calls, ensure corresponding
    // tool results stay with it by adjusting split forward.
    while split_at < messages.len() && split_at > 2
        && messages[split_at - 1].get("role").and_then(|r| r.as_str()) == Some("assistant")
        && messages[split_at - 1].get("tool_calls").is_some()
    {
        split_at += 1;
    }
    let before_tokens: usize = messages.iter()
        .map(|m| serde_json::to_string(m).unwrap_or_default().len() / 4)
        .sum();

    let older: Vec<_> = messages[2..split_at].to_vec();
    let recent = messages.split_off(split_at);

    if older.is_empty() {
        return;
    }
    let capped: Vec<_> = older.iter().take(10).cloned().collect();
    let skipped = older.len().saturating_sub(capped.len());
    if skipped > 0 {
        eprintln!("\r\x1b[K  \x1b[2m  compact: capping {} older messages → {}\x1b[0m", older.len(), capped.len());
    }
    let compactor = ConversationCompactor::new();
    let qe: &dyn LlmToolClient = client;
    match compactor.compact(&capped, None, qe).await {
        Ok(summary) => {
            let commands_note = if recent_commands.is_empty() {
                String::new()
            } else {
                let mut note = String::from("\n\n[已执行的命令 — 不要重复执行，这些探索已完成]\n");
                for (i, cmd) in recent_commands.iter().enumerate() {
                    note.push_str(&format!("{}. {}\n", i + 1, cmd));
                }
                recent_commands.clear();
                note
            };

            let mut new_messages = vec![messages[0].clone(), messages[1].clone()];
            new_messages.push(serde_json::json!({
                "role": "user",
                "content": format!("[对话上下文摘要]\n{}{}", summary, commands_note),
            }));
            new_messages.extend(recent);
            let after_tokens: usize = new_messages.iter()
                .map(|m| serde_json::to_string(m).unwrap_or_default().len() / 4)
                .sum();
            *messages = new_messages;
            eprintln!("\r\x1b[K  \x1b[2m🗜️ 压缩完成 ({} turns, {}→{} tokens)\x1b[0m", older.len(), before_tokens, after_tokens);
        }
        Err(e) => {
            eprintln!("\r\x1b[K  \x1b[2m⚠ compact failed: {}\x1b[0m", e);
        }
    }
}

/// Check if a usable bash (with full Unix tools) is available.
fn has_usable_bash() -> bool {
    #[cfg(target_os = "windows")]
    {
        let known = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\msys64\usr\bin\bash.exe",
        ];
        if known.iter().any(|p| std::path::Path::new(p).exists()) {
            return true;
        }
        if let Ok(path) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path) {
                let p = dir.join("bash.exe");
                if p.exists() && !p.to_string_lossy().contains("usr\\bin") {
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::path::Path::new("/bin/bash").exists()
    }
}
