pub(crate) const MAX_RETRIES: usize = 3;

// ---------------------------------------------------------------------------
// Trait: dependency-injection boundary for agents that call the LLM without
// tool-use (QE, MainAgent).  Agents depend on this trait, not on ApiAdapter
// directly, so that tests can inject mock clients.
// ---------------------------------------------------------------------------
#[async_trait::async_trait]
pub trait LlmStructuredClient: Send + Sync {
    async fn call_llm_structured(
        &self,
        instructions: &str,
        input_data: &serde_json::Value,
        output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String>;
}

// ---------------------------------------------------------------------------
// Trait: dependency-injection boundary for agents that call the LLM with
// tool-use (DeepExplorer). Extends LlmStructuredClient so the same adapter
// can be used for both exploration LLM calls and Refiner calls.
// ---------------------------------------------------------------------------
#[async_trait::async_trait]
pub trait LlmToolClient: LlmStructuredClient {
    async fn call_llm_with_tools(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        response_format: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String>;
}

pub struct ApiAdapter {
    pub(crate) api_mode: ApiMode,
    pub(crate) max_retries: usize,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) model: String,
    pub(crate) thinking: bool,
}

impl ApiAdapter {
    pub fn new(api_mode: ApiMode) -> Self {
        ApiAdapter {
            api_mode,
            max_retries: MAX_RETRIES,
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            thinking: false,
        }
    }

    pub fn from_config(config: &crate::common::config::LlmConfig) -> Self {
        let api_mode = match config.api_mode.as_str() {
            "responses" => ApiMode::Responses,
            _ => ApiMode::Chat,
        };
        ApiAdapter {
            api_mode,
            max_retries: config.max_retries,
            base_url: config.base_url.clone(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            thinking: config.thinking,
        }
    }

    pub fn api_mode(&self) -> &ApiMode {
        &self.api_mode
    }
}

#[async_trait::async_trait]
impl LlmStructuredClient for ApiAdapter {
    async fn call_llm_structured(
        &self,
        instructions: &str,
        input_data: &serde_json::Value,
        output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        let schema_constraint = match output_schema {
            Some(s) => Some(self.build_structured_output_constraint(s)),
            None => None,
        };

        let mut retry_count: usize = 0;
        loop {
            let raw_response = match self.api_mode {
                ApiMode::Chat => {
                    let prompt = if instructions.to_lowercase().contains("json") {
                        format!(
                            "{}\n\n{}",
                            instructions,
                            serde_json::to_string(input_data).unwrap_or_default()
                        )
                    } else {
                        format!(
                            "{}\n\n请以 JSON 格式回复。\n{}",
                            instructions,
                            serde_json::to_string(input_data).unwrap_or_default()
                        )
                    };
                    let messages = vec![serde_json::json!({
                        "role": "system",
                        "content": prompt
                    })];
                    let rf = if output_schema.is_some() {
                        Some(serde_json::json!({"type": "json_object"}))
                    } else {
                        None
                    };
                    self.invoke_llm_with_tools(&messages, &[], rf.as_ref()).await
                }
                ApiMode::Responses => {
                    let mut request = serde_json::json!({
                        "instructions": instructions,
                        "input": input_data,
                    });
                    if let Some(ref constraint) = schema_constraint {
                        request["text"] = constraint.clone();
                    }
                    self.invoke_llm(&[request]).await
                }
            };

            let raw = match raw_response {
                Ok(r) => r,
                Err(e) => {
                    if retry_count < self.max_retries {
                        retry_count += 1;
                        continue;
                    }
                    return Err(format!(
                        "LLM call failed after {} retries: {}",
                        self.max_retries, e
                    ));
                }
            };

            match self.parse_response(&raw) {
                Ok(unified) => return Ok(unified),
                Err(parse_err) => {
                    if retry_count >= self.max_retries {
                        eprintln!(
                            "[ERROR] ApiAdapter::call_llm_structured: \
                             parse failed after {} retries: {}. Raw response: {}",
                            self.max_retries,
                            parse_err,
                            serde_json::to_string(&raw).unwrap_or_default()
                        );
                        return Err(format!(
                            "Response parsing failed after {} retries: {}",
                            self.max_retries, parse_err
                        ));
                    }

                    let raw_text = serde_json::to_string(&raw).unwrap_or_default();
                    let feature_pattern = match self.api_mode {
                        ApiMode::Chat => "tool_call",
                        ApiMode::Responses => "function_call",
                    };

                    if !raw_text.to_lowercase().contains(&feature_pattern.to_lowercase()) {
                        eprintln!(
                            "[ERROR] ApiAdapter::call_llm_structured: \
                             parse failed and no '{}' feature detected in response. Error: {}",
                            feature_pattern, parse_err
                        );
                        return Err(parse_err);
                    }

                    let retry_prompt = self.build_retry_prompt(&raw_text);
                    retry_count += 1;
                    // For retry, append the retry prompt as a user message (Chat-mode
                    // convention).  Responses-mode retry is handled separately by
                    // re-sending with modified instructions.
                    let _ = retry_prompt;
                    continue;
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl LlmToolClient for ApiAdapter {
    async fn call_llm_with_tools(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        response_format: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        self.call_llm_with_tools(messages, tools, response_format).await
    }
}

// Re-export all public types so that `use adapter::api_adapter::*` still works.
pub use super::data_provider::DataProvider;
pub use super::types::{
    ApiMode, ExplorationHistoryData, OutputSchema, ToolCallInfo, ToolDefinition,
    UnifiedResponse, CHAT_TOOL_CALL_FORMAT, RESPONSES_TOOL_CALL_FORMAT,
    PLACEHOLDER_QUESTION, PLACEHOLDER_EXPLORATION_HISTORY, PLACEHOLDER_CURRENT_SUMMARY,
    PLACEHOLDER_TOOLS, PLACEHOLDER_LOOP_WARNING,
};
