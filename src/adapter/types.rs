use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiMode {
    Chat,
    Responses,
}

impl Default for ApiMode {
    fn default() -> Self {
        ApiMode::Chat
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct UnifiedResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
    pub reasoning: Option<String>,
}

/// JSON Schema object used for structured output constraints.
pub type OutputSchema = serde_json::Value;

/// JSON array of exploration history records.
/// Format varies per agent implementation (SearchStrategyAgent vs DeepExplorer).
pub type ExplorationHistoryData = serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub const CHAT_TOOL_CALL_FORMAT: &str =
    r#"在 Chat API 模式下，工具调用必须包含在 choices[0].message.tool_calls 数组中。"#;

pub const RESPONSES_TOOL_CALL_FORMAT: &str =
    r#"在 Responses API 模式下，工具调用必须在 output 数组中返回。"#;

pub const PLACEHOLDER_QUESTION: &str = "{question}";
pub const PLACEHOLDER_EXPLORATION_HISTORY: &str = "{exploration_history}";
pub const PLACEHOLDER_CURRENT_SUMMARY: &str = "{current_summary}";
pub const PLACEHOLDER_TOOLS: &str = "{tools}";
pub const PLACEHOLDER_LOOP_WARNING: &str = "{loop_warning}";
