use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use crate::adapter::api_adapter::LlmToolClient;
use crate::adapter::model::ModelAdapter;
use crate::adapter::types::ToolDefinition;
use crate::agents::conversation_compactor::ConversationCompactor;
use crate::common::config::DeepExplorerConfig;
use crate::tools::registry::ToolRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalFileRef {
    pub path: String,
    pub summary: String,
}

pub const MAX_TOOL_CALLS: usize = 75;
const TOOL_OUTPUT_MAX_CHARS: usize = 2000;

fn deserialize_line<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum LineField { Int(u64), Str(String) }
    match LineField::deserialize(d)? {
        LineField::Int(n) => Ok(n.to_string()),
        LineField::Str(s) => Ok(s),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedEvidence {
    pub file: String,
    #[serde(deserialize_with = "deserialize_line")]
    pub line: String,
    pub code_snippet: String,
    pub relevance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepExplorerResult {
    pub critical_files: Vec<CriticalFileRef>,
    pub collected_evidence: Vec<CollectedEvidence>,
    pub missing_info: String,
}

pub struct DeepExplorer {
    pub max_tool_calls: usize,
    loop_warning_threshold: usize,
    call_cache: HashSet<String>,
    consecutive_duplicates: usize,
}

impl DeepExplorer {
    pub fn new() -> Self {
        DeepExplorer {
            max_tool_calls: MAX_TOOL_CALLS,
            loop_warning_threshold: 3,
            call_cache: HashSet::new(),
            consecutive_duplicates: 0,
        }
    }

    pub fn from_config(config: &DeepExplorerConfig) -> Self {
        DeepExplorer {
            max_tool_calls: config.max_tool_calls,
            loop_warning_threshold: config.loop_warning_threshold,
            call_cache: HashSet::new(),
            consecutive_duplicates: 0,
        }
    }

    pub fn max_tool_calls(&self) -> usize {
        self.max_tool_calls
    }

    fn check_duplicate(&mut self, tool_name: &str, params_hash: &str) -> bool {
        let key = format!("{}:{}", tool_name, params_hash);
        if self.call_cache.contains(&key) {
            self.consecutive_duplicates += 1;
            true
        } else {
            self.consecutive_duplicates = 0;
            self.call_cache.insert(key);
            false
        }
    }

    fn loop_warning(&self) -> Option<String> {
        if self.consecutive_duplicates >= self.loop_warning_threshold {
            Some("⚠️ 你已连续多次执行完全相同的操作。请调整探索方向，尝试不同的工具或搜索词。".into())
        } else {
            None
        }
    }

    fn build_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "search_content".into(),
                description: "Search file contents using regex. Returns file paths and matching line numbers.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Regex pattern to search for"},
                        "file_pattern": {"type": "string", "description": "Optional file name filter (glob)"},
                        "exclude_paths": {"type": "array", "items": {"type": "string"}, "description": "Directories to exclude"}
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "search_files".into(),
                description: "Find files by glob pattern. Returns matching file paths sorted by modification time.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Glob pattern (e.g. \"**/*.rs\")"}
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "read_file".into(),
                description: "Read file content. Returns full text or specified line range.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file": {"type": "string", "description": "File path"},
                        "lines": {"type": "string", "description": "Optional line range (e.g. \"1-100\")"}
                    },
                    "required": ["file"]
                }),
            },
            ToolDefinition {
                name: "list_dir".into(),
                description: "List directory contents.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path"}
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "file_info".into(),
                description: "Get file metadata (size, modified time, etc).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file": {"type": "string", "description": "File path"}
                    },
                    "required": ["file"]
                }),
            },
            ToolDefinition {
                name: "execute_shell".into(),
                description: "Execute a read-only shell command for complex queries (find, grep -rn, wc -l, pipes).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "Shell command to execute"}
                    },
                    "required": ["command"]
                }),
            },
        ]
    }

    pub async fn execute(
        &mut self,
        question: &str,
        adapter: &dyn LlmToolClient,
        model_adapter: &dyn ModelAdapter,
        tool_registry: &ToolRegistry,
    ) -> Result<DeepExplorerResult, String> {
        let t0 = std::time::Instant::now();
        let tools = Self::build_tools();
        let tools_json = model_adapter.format_tools(&tools);

        let system_prompt = format!(
            "你是代码库深度探索专家。深入代码库，自主调用工具收集与用户问题相关的原始代码证据。\n\
             \n\
             ## 用户问题\n{}\n\
             \n\
             ## 原则\n\
             - 聚焦探索，自主调用工具收集代码证据\n\
             - 避免短期重复：不要连续多次执行完全相同的工具调用\n\
             - 适时终止：当证据足以回答用户问题时立即输出最终结果\n\
             - 禁止猜测：所有结论必须有代码证据支撑",
            question
        );

        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": format!("请探索: {}", question)}),
        ];

        let mut tool_call_count: usize = 0;
        let mut previous_summary: Option<String> = None;

        loop {
            if tool_call_count >= self.max_tool_calls {
                return Ok(DeepExplorerResult {
                    critical_files: vec![],
                    collected_evidence: vec![],
                    missing_info: "探索达到上限被强制终止".to_string(),
                });
            }
            if tool_call_count >= self.max_tool_calls - 5 {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": "[系统] 探索次数即将达到上限，请尽快整理证据输出最终结果。"
                }));
            }
            if let Some(w) = self.loop_warning() {
                messages.push(serde_json::json!({"role": "user", "content": w}));
            }

            // OpenCode-style compaction: truncate + compact when context gets large
            let total_chars: usize = messages.iter()
                .map(|m| serde_json::to_string(m).unwrap_or_default().len())
                .sum();
            if total_chars / 4 > 8000 && messages.len() > 5 {
                eprintln!("\r\x1b[K  \x1b[2m🗜️ Deep Explorer 压缩中...\x1b[0m");
                // Snapshot user_q before mutating messages — after split_off(),
                // messages only contains the older portion so get(1) would be wrong.
                let user_q = messages.get(1).cloned()
                    .unwrap_or_else(|| serde_json::json!({"role": "user", "content": ""}));
                let system = messages.remove(0);
                let keep = 3usize.min(messages.len());
                let mut split = messages.len().saturating_sub(keep);
                // Don't split in the middle of an assistant/tool pair.
                // Check both OpenAI format (role="tool") and Anthropic format (role="user" with tool_result).
                let original = split;
                while split > 0 && is_tool_result_msg(&messages[split]) {
                    split -= 1;
                }
                while split < original && split > 0 && has_tool_calls(&messages[split - 1]) {
                    split += 1;
                }
                let older: Vec<_> = messages[..split].to_vec();
                let recent = messages.split_off(split);

                if !older.is_empty() {
                    // Truncate tool outputs
                    let capped: Vec<_> = older.iter().rev().take(10).rev().cloned().collect();
                    let compactor = ConversationCompactor::new();
                    let qe: &dyn LlmToolClient = adapter;
                    match compactor.compact(&capped, previous_summary.as_deref(), qe).await {
                        Ok(summary) => {
                            messages = vec![system, user_q];
                            messages.push(serde_json::json!({
                                "role": "user",
                                "content": format!("[上下文摘要]\n{}", summary),
                            }));
                            messages.extend(recent);
                            previous_summary = Some(summary);
                            eprintln!("\r\x1b[K  \x1b[2m🗜️ Deep Explorer 压缩完成\x1b[0m");
                        }
                        Err(e) => {
                            eprintln!("\r\x1b[K  \x1b[2m⚠ DE compact failed: {}\x1b[0m", e);
                            messages.insert(0, system);
                            messages.extend(recent);
                        }
                    }
                }
            }

            let response = adapter
                .call_llm_with_tools(&messages, &tools_json, None)
                .await?;

            // Display reasoning
            if let Some(ref reason) = response.reasoning {
                let preview: String = reason.lines().take(2).collect();
                eprintln!("\r\x1b[K  \x1b[2mDeep Explorer: {}\x1b[0m", preview);
            }

            // No tool calls → LLM is done exploring
            if response.tool_calls.is_empty() {
                eprintln!("\r\x1b[K  \x1b[2mDeep Explorer 完成：{} 次工具调用，耗时 {:.1}s\x1b[0m", tool_call_count, t0.elapsed().as_secs_f64());
                return Ok(DeepExplorerResult {
                    critical_files: vec![],
                    collected_evidence: vec![],
                    missing_info: response.text.unwrap_or_default(),
                });
            }

            // Build assistant + tool messages via ModelAdapter (protocol-agnostic)
            messages.push(model_adapter.build_assistant_with_tools(&response.tool_calls));

            // Dispatch tool calls
            for tc in &response.tool_calls {
                eprintln!("\r\x1b[K  \x1b[2m⬩ Deep Explorer: {}\x1b[0m", tc.name);
                let params_str = serde_json::to_string(&tc.arguments).unwrap_or_default();
                let _is_dup = self.check_duplicate(&tc.name, &params_str);

                let result = tool_registry
                    .execute(&tc.name, tc.arguments.clone())
                    .map(|output| output.data)
                    .unwrap_or_else(|e| serde_json::json!({"success": false, "error": e.error}));

                let result_str = serde_json::to_string(&result).unwrap_or_default();
                let content = if result_str.len() > TOOL_OUTPUT_MAX_CHARS {
                    format!("{}...(truncated)", &result_str[..TOOL_OUTPUT_MAX_CHARS])
                } else {
                    result_str
                };

                messages.push(model_adapter.build_tool_result(
                    &tc.id.clone().unwrap_or_default(), &content,
                ));

                tool_call_count += 1;
            }
        }
    }
}

fn is_tool_result_msg(msg: &serde_json::Value) -> bool {
    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
    if role == "tool" { return true; }
    if role == "user" {
        if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
            return arr.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"));
        }
    }
    false
}

fn has_tool_calls(msg: &serde_json::Value) -> bool {
    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
    if role != "assistant" { return false; }
    if msg.get("tool_calls").is_some() { return true; }
    if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
        return arr.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
    }
    false
}

