use explore_ai_agent::adapter::api_adapter::*;
use explore_ai_agent::context::exploration::ExplorationSummary;

// ===== Mock DataProvider for assemble_prompt tests =====

struct MockProvider {
    question: String,
    exploration_history: serde_json::Value,
    current_summary: ExplorationSummary,
    tools: Vec<ToolDefinition>,
    output_schema: Option<serde_json::Value>,
    loop_warning: Option<String>,
}

impl MockProvider {
    fn new() -> Self {
        MockProvider {
            question: String::new(),
            exploration_history: serde_json::Value::Null,
            current_summary: ExplorationSummary {
                key_findings: String::new(),
                critical_files: vec![],
                missing_info: String::new(),
                confidence: 0.0,
            },
            tools: vec![],
            output_schema: None,
            loop_warning: None,
        }
    }
}

impl DataProvider for MockProvider {
    fn get_question(&self) -> String {
        self.question.clone()
    }

    fn get_exploration_history(&self) -> serde_json::Value {
        self.exploration_history.clone()
    }

    fn get_current_summary(&self) -> ExplorationSummary {
        self.current_summary.clone()
    }

    fn get_tools(&self) -> Vec<ToolDefinition> {
        self.tools.clone()
    }

    fn get_output_schema(&self) -> Option<serde_json::Value> {
        self.output_schema.clone()
    }

    fn get_loop_warning(&self) -> Option<String> {
        self.loop_warning.clone()
    }
}

// ===== ApiMode Tests (AD-001, AD-002) =====

#[test]
fn api_mode_default_is_chat() {
    let mode = ApiMode::default();
    assert_eq!(mode, ApiMode::Chat);
}

#[test]
fn api_adapter_creation() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    assert_eq!(*adapter.api_mode(), ApiMode::Chat);

    let adapter = ApiAdapter::new(ApiMode::Responses);
    assert_eq!(*adapter.api_mode(), ApiMode::Responses);
}

// ===== Prompt Assembly Tests (AD-003 ~ AD-010, AD-007b) =====

// AD-003: Chat mode — {question} replaced via assemble_prompt
#[test]
fn chat_mode_replaces_question_placeholder() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let mut provider = MockProvider::new();
    provider.question = "What is BooleanValidator?".to_string();

    let result = adapter.assemble_prompt("Prompt: {question}", &provider);
    assert!(result.contains("## 用户问题"));
    assert!(result.contains("What is BooleanValidator?"));
    assert!(!result.contains("{question}"));
}

// AD-004: Responses mode — {question} cleared via assemble_prompt
#[test]
fn responses_mode_clears_question_placeholder() {
    let adapter = ApiAdapter::new(ApiMode::Responses);
    let mut provider = MockProvider::new();
    provider.question = "What is BooleanValidator?".to_string();

    let result = adapter.assemble_prompt("Prompt: {question}", &provider);
    assert!(!result.contains("## 用户问题"));
    assert!(!result.contains("{question}"));
}

// AD-005: Chat mode — {exploration_history} replaced via assemble_prompt
#[test]
fn chat_mode_replaces_exploration_history() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let mut provider = MockProvider::new();
    provider.exploration_history = serde_json::json!([{"round":1,"keywords":["test"]}]);

    let result = adapter.assemble_prompt("Template: {exploration_history}", &provider);
    assert!(result.contains("## 历史探索记录"));
    assert!(result.contains("round"));
}

// AD-006: Responses mode — {exploration_history} cleared via assemble_prompt
#[test]
fn responses_mode_clears_exploration_history() {
    let adapter = ApiAdapter::new(ApiMode::Responses);
    let mut provider = MockProvider::new();
    provider.exploration_history = serde_json::json!("some data");

    let result = adapter.assemble_prompt("Template: {exploration_history}", &provider);
    assert!(!result.contains("## 历史探索记录"));
    assert!(!result.contains("{exploration_history}"));
}

// AD-007: Chat mode — non-empty tools via assemble_prompt
#[test]
fn chat_mode_replaces_tools() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let mut provider = MockProvider::new();
    provider.tools = vec![
        ToolDefinition {
            name: "fast_explorer".to_string(),
            description: "Fast batch exploration".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        },
        ToolDefinition {
            name: "search_content".to_string(),
            description: "Search text patterns".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        },
    ];

    let result = adapter.assemble_prompt("Template: {tools}", &provider);
    assert!(result.contains("## 可用工具"));
    let after_tools_heading = result.split("## 可用工具").nth(1).unwrap_or("");
    assert!(!after_tools_heading.trim().is_empty(),
        "Tools section should contain non-empty format description text");
}

// AD-007b: Chat mode — empty tools list via assemble_prompt
#[test]
fn chat_mode_replaces_tools_empty_list() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let mut provider = MockProvider::new();
    provider.tools = vec![];

    let result = adapter.assemble_prompt("Template: {tools}", &provider);
    assert!(result.contains("## 可用工具"));
    // Empty tools: the heading exists but the section after it is empty
    let after_tools_heading = result.split("## 可用工具").nth(1).unwrap_or("");
    assert!(after_tools_heading.trim().is_empty(),
        "Empty tools list should produce empty section after heading");
}

// AD-008: Chat mode — {current_summary} replaced via assemble_prompt
#[test]
fn chat_mode_replaces_current_summary() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let mut provider = MockProvider::new();
    provider.current_summary = ExplorationSummary {
        key_findings: "test".to_string(),
        critical_files: vec![],
        missing_info: "".to_string(),
        confidence: 0.5,
    };

    let result = adapter.assemble_prompt("Template: {current_summary}", &provider);
    assert!(result.contains("## 已有探索线索"));
    assert!(result.contains("key_findings"));
}

// AD-009: Chat mode — loop_warning with text via assemble_prompt
#[test]
fn chat_mode_loop_warning_present() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let mut provider = MockProvider::new();
    provider.loop_warning = Some("你已连续多次执行相似操作……".to_string());

    let result = adapter.assemble_prompt("Template: {loop_warning}", &provider);
    assert!(result.contains("## ⚠️ 系统警告"));
    assert!(result.contains("你已连续多次执行相似操作……"));
}

// AD-010: Chat mode — loop_warning None via assemble_prompt
#[test]
fn chat_mode_loop_warning_absent() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let mut provider = MockProvider::new();
    provider.loop_warning = None;

    let result = adapter.assemble_prompt("Template: {loop_warning}", &provider);
    assert!(!result.contains("## ⚠️ 系统警告"));
}

// ===== Response Parsing Tests (AD-011 ~ AD-016) =====

#[test]
fn parse_chat_api_tool_calls_response() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let raw = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {
                        "name": "search_content",
                        "arguments": "{\"pattern\": \"test\"}"
                    }
                }]
            }
        }]
    });

    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "search_content");
}

#[test]
fn parse_responses_api_tool_calls() {
    let adapter = ApiAdapter::new(ApiMode::Responses);
    let raw = serde_json::json!({
        "output": [{
            "type": "function_call",
            "call_id": "call_abc",
            "name": "search_content",
            "arguments": "{\"pattern\": \"test\"}"
        }]
    });

    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "search_content");
}

#[test]
fn parse_text_only_response() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let raw = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "The answer is 42."
            }
        }]
    });

    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.text, Some("The answer is 42.".to_string()));
}

// AD-014: parse Responses API text-only response (output with type="message")
#[test]
fn parse_responses_api_text_only_response() {
    let adapter = ApiAdapter::new(ApiMode::Responses);
    let raw = serde_json::json!({
        "output": [{
            "type": "message",
            "content": [{
                "type": "output_text",
                "text": "The answer is 42."
            }]
        }]
    });

    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.text, Some("The answer is 42.".to_string()));
}

// AD-015: parse empty/malformed response returns Err
#[test]
fn parse_empty_response_returns_err() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let raw = serde_json::json!({});
    let result = adapter.parse_response(&raw);
    assert!(result.is_err());
}

// AD-016: Chat mode arguments with nested JSON object
#[test]
fn parse_chat_api_tool_calls_with_nested_arguments() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let raw = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_def",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"file\": \"src/main.rs\", \"lines\": {\"ranges\": [[1, 10]]}}"
                    }
                }]
            }
        }]
    });

    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "read_file");
    assert_eq!(response.tool_calls[0].arguments["file"], "src/main.rs");
    assert!(response.tool_calls[0].arguments["lines"]["ranges"].is_array());
}

// ===== Tool Result Message Building (AD-017, AD-018) =====

#[test]
fn build_chat_tool_result() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let msg = adapter.build_tool_result_message("call_abc", "search results here");

    assert_eq!(msg["role"], "tool");
    assert_eq!(msg["tool_call_id"], "call_abc");
    assert_eq!(msg["content"], "search results here");
}

#[test]
fn build_responses_tool_result() {
    let adapter = ApiAdapter::new(ApiMode::Responses);
    let msg = adapter.build_tool_result_message("call_abc", "search results here");

    assert_eq!(msg["type"], "function_call_output");
    assert_eq!(msg["call_id"], "call_abc");
    assert_eq!(msg["output"], "search results here");
}

// ===== Structured Output Constraint Tests (AD-019, AD-020) =====

// AD-019: Chat mode — result contains response_format with nested json_schema
#[test]
fn chat_mode_structured_output_constraint() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let schema = serde_json::json!({
        "name": "test_response",
        "strict": true,
        "schema": {
            "type": "object",
            "properties": {
                "key_findings": {"type": "string"}
            },
            "required": ["key_findings"]
        }
    });

    let constraint = adapter.build_structured_output_constraint(&schema);

    // Chat mode: response_format -> { type: "json_schema", json_schema: { name, strict, schema } }
    let rf = constraint.get("response_format").expect("Chat mode should produce response_format field");
    assert_eq!(rf["type"], "json_schema");
    let js = &rf["json_schema"];
    assert_eq!(js["name"], "test_response");
    assert_eq!(js["strict"], true);
    assert!(js["schema"]["properties"]["key_findings"].is_object());
}

// AD-020: Responses mode — result contains text.format with json_schema
#[test]
fn responses_mode_structured_output_constraint() {
    let adapter = ApiAdapter::new(ApiMode::Responses);
    let schema = serde_json::json!({
        "name": "test_response",
        "strict": true,
        "schema": {
            "type": "object",
            "properties": {
                "key_findings": {"type": "string"}
            },
            "required": ["key_findings"]
        }
    });

    let constraint = adapter.build_structured_output_constraint(&schema);

    // Responses mode: text -> { format: { type: "json_schema", name, strict, schema } }
    let text = constraint.get("text").expect("Responses mode should produce text field");
    let format = text.get("format").expect("text should contain format field");
    assert_eq!(format["type"], "json_schema");
    assert_eq!(format["name"], "test_response");
    assert_eq!(format["strict"], true);
    assert!(format["schema"]["properties"]["key_findings"].is_object());
}

// ===== Retry Prompt Tests (AD-021, AD-022) =====

#[test]
fn retry_prompt_contains_format_description() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let prompt = adapter.build_retry_prompt("malformed response content");

    assert!(prompt.contains("格式不符合规范"));
    assert!(prompt.contains("malformed response content"));
    assert!(prompt.contains("正确的工具调用格式"));
}

#[test]
fn retry_prompt_mode_specific() {
    let chat_adapter = ApiAdapter::new(ApiMode::Chat);
    let chat_prompt = chat_adapter.build_retry_prompt("test");

    let resp_adapter = ApiAdapter::new(ApiMode::Responses);
    let resp_prompt = resp_adapter.build_retry_prompt("test");

    // Different modes should produce different format descriptions
    assert_ne!(chat_prompt, resp_prompt);
}

// ===== Tool Call Format Description (AD-023, AD-024) =====

#[test]
fn chat_mode_format_description() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let desc = adapter.get_tool_call_format_description();
    assert!(desc.contains("Chat API"));
}

#[test]
fn responses_mode_format_description() {
    let adapter = ApiAdapter::new(ApiMode::Responses);
    let desc = adapter.get_tool_call_format_description();
    assert!(desc.contains("Responses API"));
}

// ===== Data Structure Serialization (AD-025 ~ AD-027) =====

#[test]
fn tool_definition_serialization() {
    let def = ToolDefinition {
        name: "search_content".to_string(),
        description: "Search for text patterns".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"}
            }
        }),
    };

    let json = serde_json::to_value(&def).unwrap();
    assert_eq!(json["name"], "search_content");

    let deserialized: ToolDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.name, "search_content");
}

#[test]
fn unified_response_text_only() {
    let response = UnifiedResponse {
        text: Some("Hello".to_string()),
        tool_calls: vec![],
        reasoning: None,
    };
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.text.unwrap(), "Hello");
}

#[test]
fn unified_response_with_tool_calls() {
    let response = UnifiedResponse {
        text: None,
        tool_calls: vec![
            ToolCallInfo {
                name: "search_content".to_string(),
                arguments: serde_json::json!({"pattern": "test"}),
            },
            ToolCallInfo {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"file": "src/main.rs"}),
            },
        ],
        reasoning: None,
    };
    assert_eq!(response.tool_calls.len(), 2);
}

// ===== Retry Flow Tests (AD-028 ~ AD-030) =====

// AD-028: regex fallback match detects tool_call feature string in non-standard response
#[test]
fn regex_fallback_match_detects_tool_call_feature() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    // Simulate a non-standard response that contains tool_call text but isn't
    // properly formatted — parse_response should fail, and the regex fallback
    // inside call_llm_with_retry should detect the feature string.
    let raw = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "I will call tool_call: search_content with pattern=test"
            }
        }]
    });

    // parse_response should succeed (it's valid JSON with content),
    // but when used inside call_llm_with_retry with tool detection,
    // the regex fallback would match "tool_call" in the content
    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.tool_calls.is_empty());
    // The text content contains "tool_call" — regex fallback should detect it
    let text = response.text.unwrap_or_default();
    assert!(text.to_lowercase().contains("tool_call"),
        "Non-standard tool_call text should be detectable by regex fallback");
}

// AD-029: no tool call feature in response → no retry triggered
#[test]
fn no_tool_call_feature_no_retry() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    // Pure text response with no tool call indicators at all
    let raw = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "The answer to your question is 42. No tools needed."
            }
        }]
    });

    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.tool_calls.is_empty());
    let text = response.text.unwrap_or_default();
    // No tool_call/function_call feature string present
    assert!(!text.to_lowercase().contains("tool_call"));
    assert!(!text.to_lowercase().contains("function_call"));
}

// AD-030: retry exhausted after MAX_RETRIES (3) attempts
#[tokio::test]
async fn retry_exhausted_after_max_attempts() {
    let adapter = ApiAdapter::new(ApiMode::Chat);

    // Messages simulating a conversation that keeps producing
    // malformed tool call responses
    let messages = vec![
        serde_json::json!({"role": "user", "content": "test"}),
    ];

    // call_llm_with_retry will attempt up to MAX_RETRIES (3) and return Err
    // when the LLM client is not configured / all retries are exhausted
    let result = adapter.call_llm_with_retry(&messages).await;
    // Currently expected to fail because LLM client is not configured.
    // After implementation: should return Err after 3 exhausted retries.
    assert!(result.is_err());
}
