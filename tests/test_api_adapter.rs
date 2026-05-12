use explore_ai_agent::adapter::api_adapter::*;

// ===== ApiMode Tests =====

#[test]
fn api_mode_default_is_chat() {
    let mode = ApiMode::default();
    assert_eq!(mode, ApiMode::Chat);
}

#[test]
fn api_adapter_creation() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    assert_eq!(*adapter.api_mode(), ApiMode::Chat);
}

// ===== Response Parsing Tests =====

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

#[test]
fn parse_empty_response_returns_err() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let raw = serde_json::json!({});
    let result = adapter.parse_response(&raw);
    assert!(result.is_err());
}

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

// ===== Tool Result Message Building =====

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

// ===== Structured Output Constraint Tests =====

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

    let rf = constraint.get("response_format").expect("Chat mode should produce response_format field");
    assert_eq!(rf["type"], "json_schema");
    let js = &rf["json_schema"];
    assert_eq!(js["name"], "test_response");
    assert_eq!(js["strict"], true);
    assert!(js["schema"]["properties"]["key_findings"].is_object());
}

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

    let text = constraint.get("text").expect("Responses mode should produce text field");
    let format = text.get("format").expect("text should contain format field");
    assert_eq!(format["type"], "json_schema");
    assert_eq!(format["name"], "test_response");
    assert_eq!(format["strict"], true);
    assert!(format["schema"]["properties"]["key_findings"].is_object());
}

// ===== Retry Prompt Tests =====

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

    assert_ne!(chat_prompt, resp_prompt);
}

// ===== Tool Call Format Description =====

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

// ===== Data Structure Serialization =====

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
                id: None,
                name: "search_content".to_string(),
                arguments: serde_json::json!({"pattern": "test"}),
            },
            ToolCallInfo {
                id: None,
                name: "read_file".to_string(),
                arguments: serde_json::json!({"file": "src/main.rs"}),
            },
        ],
        reasoning: None,
    };
    assert_eq!(response.tool_calls.len(), 2);
}

// ===== Retry Flow Tests =====

#[test]
fn regex_fallback_match_detects_tool_call_feature() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
    let raw = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "I will call tool_call: search_content with pattern=test"
            }
        }]
    });

    let result = adapter.parse_response(&raw);
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.tool_calls.is_empty());
    let text = response.text.unwrap_or_default();
    assert!(text.to_lowercase().contains("tool_call"),
        "Non-standard tool_call text should be detectable by regex fallback");
}

#[test]
fn no_tool_call_feature_no_retry() {
    let adapter = ApiAdapter::new(ApiMode::Chat);
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
    assert!(!text.to_lowercase().contains("tool_call"));
    assert!(!text.to_lowercase().contains("function_call"));
}

#[tokio::test]
async fn retry_exhausted_after_max_attempts() {
    let adapter = ApiAdapter::new(ApiMode::Chat);

    let messages = vec![
        serde_json::json!({"role": "user", "content": "test"}),
    ];

    let result = adapter.call_llm_with_retry(&messages).await;
    assert!(result.is_err());
}
