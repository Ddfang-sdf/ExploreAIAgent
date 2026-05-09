use tokio::sync::mpsc;
use crate::adapter::api_adapter::{LlmStructuredClient, LlmToolClient};

#[derive(Debug, Clone)]
pub enum SseEvent { Thinking(String), Answer(String), Done }
pub static SSE_TX: std::sync::Mutex<Option<mpsc::UnboundedSender<SseEvent>>> = std::sync::Mutex::new(None);
pub fn sse_enable() -> mpsc::UnboundedReceiver<SseEvent> { let (tx, rx) = mpsc::unbounded_channel(); *SSE_TX.lock().unwrap() = Some(tx); rx }
pub fn sse_disable() { *SSE_TX.lock().unwrap() = None; }
pub fn sse_send(e: SseEvent) { if let Some(tx) = SSE_TX.lock().unwrap().as_ref() { let _ = tx.send(e); } }

// ============================================================================
// v1.2: Trait definitions for dependency injection
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

pub struct MainAgent;

impl MainAgent {
    pub fn new() -> Self {
        MainAgent
    }

    /// v1.2: Decision loop — JSON communication with LLM, tool dispatch
    pub async fn run(
        &self,
        question: &str,
        conversation_context: &str,
        fast_explore: &dyn FastExploreExecutor,
        de: Option<&dyn DeepExploreExecutor>,
        shell: &dyn ShellExecutor,
        client: &dyn LlmToolClient,
    ) -> Result<String, String> {
        let enable_de = de.is_some();
        let system_prompt_raw = Self::assemble_prompt();
        let system_prompt = if enable_de {
            system_prompt_raw
        } else {
            // Strip deep_explore section when DE is disabled
            if let Some(start) = system_prompt_raw.find("### deep_explore") {
                let after_start = &system_prompt_raw[start..];
                if let Some(end_offset) = after_start.find("\n### execute_shell") {
                    format!("{}{}", &system_prompt_raw[..start], &after_start[end_offset..])
                } else {
                    system_prompt_raw
                }
            } else {
                system_prompt_raw
            }
        };
        let system_prompt = system_prompt
            .replace("{shell_info}", &Self::shell_info())
            .replace("{shell_commands}", &Self::shell_commands());
        let user_content = if conversation_context.is_empty() {
            question.to_string()
        } else {
            format!("{}\n{}", conversation_context, question)
        };

        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": user_content}),
        ];

        let _schema = Self::action_schema();
        let mut parse_retries: usize = 0;
        const MAX_PARSE_RETRIES: usize = 2;
        let mut first_call = true;

        loop {
            if first_call {
                eprintln!("⏳ 正在分析问题...");
                first_call = false;
            }
            let rf = serde_json::json!({"type": "json_object"});
            let response = client
                .call_llm_with_tools(&messages, &[], Some(&rf))
                .await?;

            let text = match response.text {
                Some(t) if !t.is_empty() => t,
                _ => {
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": "你的回复为空。请按照 JSON 格式输出：{\"action\": \"answer\", ...} 或 {\"action\": \"tool_call\", ...}"
                        }));
                        parse_retries += 1;
                        continue;
                    }
                    return Err("Empty response from LLM".to_string());
                }
            };

            // Parse JSON action
            let action_json: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": format!("你的回复不是合法 JSON。请严格按照格式输出。错误: {}", e),
                        }));
                        parse_retries += 1;
                        continue;
                    }
                    return Err(format!("JSON parse retry exhausted: {}", e));
                }
            };
            parse_retries = 0;

            let action = action_json.get("action").and_then(|v| v.as_str()).unwrap_or("");

            match action {
                "answer" => {
                    let final_response = action_json
                        .get("final_response")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    return Ok(final_response.to_string());
                }
                "tool_call" => {
                    let tool = action_json.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                    match tool {
                        "fast_explore" => {
                            eprintln!("\r\x1b[K🔍 正在搜索代码库...");
                            sse_send(SseEvent::Thinking("🔍 正在搜索代码库...".into()));
                            let keywords: Vec<String> = action_json
                                .get("arguments")
                                .and_then(|a| a.get("keywords"))
                                .and_then(|k| k.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default();

                            match fast_explore.execute(&keywords).await {
                                Ok(result) => {
                                    let result_str = serde_json::to_string(&result).unwrap_or_default();
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("fast_explore 返回结果:\n{}", result_str),
                                    }));
                                }
                                Err(e) => {
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("fast_explore 执行失败: {}", e),
                                    }));
                                }
                            }
                        }
                        "deep_explore" => {
                            if let Some(de) = de {
                                eprintln!("\r\x1b[K🔍 正在深入探索代码...");
                                sse_send(SseEvent::Thinking("🔍 正在深入探索代码...".into()));
                                let de_question = action_json
                                    .get("arguments")
                                    .and_then(|a| a.get("question"))
                                    .and_then(|q| q.as_str())
                                    .unwrap_or(question);
                                let summary = action_json.get("arguments").and_then(|a| a.get("current_summary"));

                                match de.execute(de_question, summary).await {
                                    Ok(result) => {
                                        let result_str = serde_json::to_string(&result).unwrap_or_default();
                                        messages.push(serde_json::json!({
                                            "role": "user",
                                            "content": format!("deep_explore 返回结果:\n{}", result_str),
                                        }));
                                    }
                                    Err(e) => {
                                        messages.push(serde_json::json!({
                                            "role": "user",
                                            "content": format!("deep_explore 执行失败: {}", e),
                                        }));
                                    }
                                }
                            } else {
                                messages.push(serde_json::json!({
                                    "role": "user",
                                    "content": "deep_explore 当前不可用。可用工具为 fast_explore、execute_shell。",
                                }));
                            }
                        }
                        "execute_shell" => {
                            let reasoning = action_json.get("reasoning").and_then(|v| v.as_str()).unwrap_or("");
                            let thinking = if reasoning.is_empty() { "执行 Shell 命令".to_string() } else { reasoning.to_string() };
                            eprintln!("\r\x1b[K  \x1b[2m⬩ {}\x1b[0m", thinking);
                            sse_send(SseEvent::Thinking(thinking));
                            let command = action_json
                                .get("arguments")
                                .and_then(|a| a.get("command"))
                                .and_then(|c| c.as_str())
                                .unwrap_or("");
                            match shell.execute(command).await {
                                Ok(result) => {
                                    let result_str = serde_json::to_string(&result).unwrap_or_default();
                                    let preview: String = result_str.chars().take(200).collect();
                                    eprintln!("\r\x1b[K  \x1b[2m⚡ ok: {} → {}\x1b[0m", command.chars().take(80).collect::<String>(), preview);
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("execute_shell 返回结果:\n{}", result_str),
                                    }));
                                }
                                Err(e) => {
                                    eprintln!("\r\x1b[K  \x1b[2m⚠ fail: {} | cmd: {}\x1b[0m", e, command);
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("execute_shell 执行失败: {} | 命令: {}", e, command),
                                    }));
                                }
                            }
                        }
                        _ => {
                            messages.push(serde_json::json!({
                                "role": "user",
                                "content": "未知工具。可用工具为 fast_explore、deep_explore、execute_shell。",
                            }));
                        }
                    }
                }
                _ => {
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": "缺少 action 字段。请输出 {\"action\": \"answer\", ...} 或 {\"action\": \"tool_call\", ...}",
                        }));
                        parse_retries += 1;
                    } else {
                        return Err("Missing action field after retries".to_string());
                    }
                }
            }
        }
    }

    pub fn action_schema() -> serde_json::Value {
        serde_json::json!({
            "name": "main_agent_action",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["answer", "tool_call"]},
                    "final_response": {"type": "string"},
                    "tool": {"type": "string", "enum": ["fast_explore", "deep_explore", "execute_shell"]},
                    "arguments": {"type": "object"}
                },
                "required": ["action"]
            }
        })
    }

    #[deprecated]
    pub fn new_legacy() -> Self {
        MainAgent
    }

    /// Extract the answer from LLM response text.
    /// Tries in order: <final_response> tags, JSON "final_response" field, raw text.
    fn extract_answer(raw: &str) -> String {
        // Try <final_response>...</final_response> tags
        if let Some(start) = raw.find("<final_response>") {
            let after_tag = &raw[start + "<final_response>".len()..];
            if let Some(end) = after_tag.find("</final_response>") {
                return after_tag[..end].trim().to_string();
            }
        }

        // Try JSON: parse and look for "final_response" key
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(text) = json.get("final_response").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }

        raw.trim().to_string()
    }

    pub async fn generate_answer(
        &self,
        user_question: &str,
        conversation_context: &str,
        exploration_data: &serde_json::Value,
        client: &dyn LlmStructuredClient,
    ) -> Result<String, String> {
        let input_data = serde_json::json!({
            "user_question": user_question,
            "conversation_context": conversation_context,
            "exploration_data": exploration_data,
        });

        let instructions = Self::assemble_prompt();

        let response = client
            .call_llm_structured(&instructions, &input_data, None)
            .await?;

        match response.text {
            Some(text) if !text.is_empty() => Ok(Self::extract_answer(&text)),
            _ => Err("Empty response from LLM".to_string()),
        }
    }


    pub fn assemble_prompt() -> String {
        String::from(
            "你是探索者（Explore AI Agent），一个专业的代码库探索助手。\n\
             你的工作方式是：理解用户问题，必要时调用代码库搜索工具获取信息，基于搜索结果回答用户。\n\
             \n\
             ## 可用工具\n\
             \n\
             你可以调用以下三个工具。工具的具体输入输出格式由系统控制，以下是它们的能力描述和数据结构：\n\
             \n\
             ### fast_explore — 快速扫描代码库\n\
             \n\
             根据你设计的关键词批量搜索代码库，返回线索摘要和置信度评分。\n\
             \n\
             | 项目 | 说明 |\n\
             |:---|:---|\n\
             | 输入 | keywords（字符串数组）：2-5 个搜索关键词。你需要自己设计关键词——从用户问题中提取核心概念，中英文兼顾 |\n\
             | 输出 | matches（搜索结果）、key_findings（核心发现）、critical_files（关键文件列表）、confidence（置信度 0.0~1.0） |\n\
             | 适用 | 可选用。首次探索时快速了解项目中与问题相关的模块分布，帮你找到大方向 |\n\
             | 限制 | 关键词匹配，覆盖面有限。可能遗漏重要代码，不可替代 deep_explore |\n\
             \n\
             输出示例：\n\
             {\"matches\": [...], \"key_findings\": \"回测模块在 backtest/engine.py 中实现\", \"critical_files\": [{\"path\": \"src/backtest/engine.py\", \"summary\": \"回测引擎核心\"}], \"confidence\": 0.8}\n\
             \n\
             ### deep_explore — 深度代码探索\n\
             \n\
             深入阅读代码文件，精确定位代码证据。\n\
             \n\
             | 项目 | 说明 |\n\
             |:---|:---|\n\
             | 输入 | question（字符串）：要调查的问题。current_summary（对象，可选）：已有的探索线索摘要 |\n\
             | 输出 | critical_files（相关文件及说明）、collected_evidence（代码证据列表，每条含 file、line、code_snippet、relevance）、missing_info（缺失信息） |\n\
             | 适用 | 主力探索工具。直接搜索代码库、阅读文件、追溯调用链、收集代码证据。任何时候需要深入调查都可以直接调用 |\n\
             | 限制 | 耗时较长（内部最多 75 次操作）。deep_explore 已穷尽搜索，结果即为该问题可获得的全部证据，不应质疑或重试 |\n\
             \n\
             输出示例：\n\
             {\"critical_files\": [{\"path\": \"src/backtest/engine.py\", \"summary\": \"回测引擎核心\"}], \"collected_evidence\": [{\"file\": \"src/backtest/engine.py\", \"line\": \"142-158\", \"code_snippet\": \"def run_backtest...\", \"relevance\": \"回测主循环\"}], \"missing_info\": \"无\"}\n\
             \n\
             ### execute_shell — 执行只读 Shell 命令\n\
             \n\
             当前 Shell：{shell_info}。可用命令：{shell_commands}（sed 禁止 -i）。\n\
             grep 搜索代码、find 查文件、awk/sed 文本处理、wc 统计、管道组合过滤。\n\
             \n\
             | 项目 | 说明 |\n\
             |:---|:---|\n\
             | 输入 | command（字符串）：只读 Shell 命令 |\n\
             | 输出 | success（是否成功）、output。失败时含 error |\n\
             | 限制 | 禁止 > 重定向、tee、rm mv cp mkdir 等写入操作。管道命令必须在白名单内。output 最多 50KB（约 2000 行），超出丢弃 |\n\
             \n\
             ## 规则\n\
             \n\
             1. 任何关于代码库的问题，必须先探索再回答。严禁在未探索的情况下猜测或说\"信息不足\"。\n\
             2. 纯问候（\"你好\"、\"谢谢\"、\"再见\"）或追问刚探索过的话题（\"再详细说说\"）可不调工具直接回答。\n\
             3. 严禁编造代码细节。若探索后仍证据不足，如实告知。\n\
             4. deep_explore 输出的 missing_info 字段不作为重试触发条件。missing_info 表示该次搜索未覆盖的盲区，不影响已收集证据的有效性。\n\
             \n\
             ## 通信协议\n\
             \n\
             你与系统之间通过 JSON 通信。每次回复必须是合法的 JSON 对象，action 字段决定操作类型：\n\
             \n\
             直接回答用户时：\n\
             {\"action\": \"answer\", \"final_response\": \"答案内容\"}\n\
             \n\
             调用工具时：\n\
             {\"action\": \"tool_call\", \"tool\": \"fast_explore\", \"arguments\": {\"keywords\": [\"回测\", \"backtest\", \"引擎\"]}}\n\
             {\"action\": \"tool_call\", \"tool\": \"deep_explore\", \"arguments\": {\"question\": \"用户问题\", \"current_summary\": {...}}}\n\
             {\"action\": \"tool_call\", \"tool\": \"execute_shell\", \"arguments\": {\"command\": \"find . -name '*.rs' | wc -l\"}, \"reasoning\": \"你的判断依据\"}}\n\
             \n\
             注意：只输出 JSON，不要包裹任何标记或解释文字。如果你的回答不符合 JSON 要求，系统将强制你重新回答，请务必按照通信协议规范的返回 JSON！",
        )
    }

    pub fn shell_info() -> String {
        if cfg!(target_os = "windows") {
            if has_usable_bash() {
                "bash (Windows)".to_string()
            } else {
                "cmd.exe (Windows)".to_string()
            }
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
}

/// Check if a usable bash (with full Unix tools) is available.
/// Mirrors the logic in execute_shell::discover_shell().
fn has_usable_bash() -> bool {
    #[cfg(target_os = "windows")]
    {
        // Same hardcoded paths as discover_shell()
        let known = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\msys64\usr\bin\bash.exe",
        ];
        if known.iter().any(|p| std::path::Path::new(p).exists()) {
            return true;
        }
        // PATH scan: skip usr\bin bash (no grep/awk), accept bin/bash
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
