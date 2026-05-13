mod openai_chat;
mod anthropic_messages;

use crate::adapter::types::{ToolDefinition, UnifiedResponse};

/// Model adapter abstracts differences between LLM API standards
/// (OpenAI Chat Completions, Anthropic Messages).
///
/// Models that implement one of these API standards are supported;
/// models that don't are not.
pub trait ModelAdapter: Send + Sync {
    /// Human-readable identifier for this adapter
    fn adapter_id(&self) -> &str;

    /// Convert ToolDefinitions to the API-native `tools` array format.
    fn format_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value>;

    /// Build the HTTP request body.
    fn build_request_body(
        &self,
        model: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        response_format: Option<&serde_json::Value>,
    ) -> serde_json::Value;

    /// Parse raw API response into UnifiedResponse.
    fn parse_response(&self, raw: &serde_json::Value) -> Result<UnifiedResponse, String>;

    /// Extract the full assistant message from raw response.
    /// Must preserve all model-specific fields (e.g. reasoning_details for MiniMax).
    fn build_assistant_message(&self, raw: &serde_json::Value) -> Result<serde_json::Value, String>;

    /// API endpoint path for this protocol (e.g. "/chat/completions", "/messages").
    fn api_path(&self) -> &str;
}

pub use openai_chat::OpenAiChatAdapter;
pub use anthropic_messages::AnthropicMessagesAdapter;
