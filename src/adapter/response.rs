use super::api_adapter::ApiAdapter;
use super::types::{ApiMode, ToolCallInfo, UnifiedResponse};

impl ApiAdapter {
    pub fn parse_response(
        &self,
        raw_response: &serde_json::Value,
    ) -> Result<UnifiedResponse, String> {
        match self.api_mode {
            ApiMode::Chat => self.parse_chat_response(raw_response),
            ApiMode::Responses => self.parse_responses_response(raw_response),
        }
    }

    fn parse_chat_response(
        &self,
        raw: &serde_json::Value,
    ) -> Result<UnifiedResponse, String> {
        let message = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .ok_or_else(|| "Missing choices[0].message in Chat API response".to_string())?;

        let mut tool_calls = Vec::new();
        let mut text: Option<String> = None;

        if let Some(tc_array) = message.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tc_array {
                let func = match tc.get("function") {
                    Some(f) => f,
                    None => continue,
                };
                let name = match func.get("name").and_then(|n| n.as_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let args_str = match func.get("arguments").and_then(|a| a.as_str()) {
                    Some(a) => a,
                    None => continue,
                };
                let arguments: serde_json::Value = match serde_json::from_str(args_str) {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!(
                            "[WARN] ApiAdapter::parse_chat_response: \
                             failed to deserialize arguments for tool '{}', skipping",
                            name
                        );
                        continue;
                    }
                };
                tool_calls.push(ToolCallInfo { name, arguments });
            }
        }

        if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                text = Some(content.to_string());
            }
        }

        Ok(UnifiedResponse { text, tool_calls })
    }

    fn parse_responses_response(
        &self,
        raw: &serde_json::Value,
    ) -> Result<UnifiedResponse, String> {
        let output = raw
            .get("output")
            .and_then(|o| o.as_array())
            .ok_or_else(|| "Missing output array in Responses API response".to_string())?;

        let mut tool_calls = Vec::new();
        let mut text_parts: Vec<String> = Vec::new();

        for item in output {
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match item_type {
                "function_call" => {
                    let name = match item.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    let args_str = match item.get("arguments").and_then(|a| a.as_str()) {
                        Some(a) => a,
                        None => continue,
                    };
                    let arguments: serde_json::Value = match serde_json::from_str(args_str) {
                        Ok(v) => v,
                        Err(_) => {
                            eprintln!(
                                "[WARN] ApiAdapter::parse_responses_response: \
                                 failed to deserialize arguments for tool '{}', skipping",
                                name
                            );
                            continue;
                        }
                    };
                    tool_calls.push(ToolCallInfo { name, arguments });
                }
                "message" => {
                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                    text_parts.push(t.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let text = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };

        Ok(UnifiedResponse { text, tool_calls })
    }
}
