use super::api_adapter::ApiAdapter;
use super::types::{ApiMode, UnifiedResponse};

impl ApiAdapter {
    pub fn build_retry_prompt(
        &self,
        previous_response: &str,
    ) -> String {
        let format_desc = self.get_tool_call_format_description();
        format!(
            "你的上一次回复格式不符合规范，未能正确识别其中的工具调用指令。\
             请严格按照以下格式要求，重新生成你的回复。\n\n\
             ## 正确的工具调用格式\n\
             {}\n\n\
             ## 你的错误回复\n\
             {}\n\n\
             ## 要求\n\
             请根据上述正确格式，将你原本想要执行的操作重新输出。\
             如果原本没有打算调用工具，请明确表示你希望直接回复文本。",
            format_desc, previous_response
        )
    }

    pub async fn call_llm_with_retry(
        &self,
        messages: &[serde_json::Value],
    ) -> Result<UnifiedResponse, String> {
        self.call_llm_with_tools(messages, &[], None).await
    }

    pub async fn call_llm_with_tools(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        response_format: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        let mut current_messages: Vec<serde_json::Value> = messages.to_vec();
        let mut retry_count: usize = 0;
        let max_retries = self.max_retries;

        loop {
            let raw_response = self.invoke_llm_with_tools(&current_messages, tools, response_format).await;
            let raw = match &raw_response {
                Ok(r) => r.clone(),
                Err(e) => {
                    if retry_count < max_retries {
                        retry_count += 1;
                        let delay_secs = (1u64 << retry_count).min(8);
                        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                        continue;
                    }
                    return Err(format!("LLM call failed after {} retries: {}", max_retries, e));
                }
            };

            let parse_result = self.parse_response(&raw);

            match parse_result {
                Ok(unified) => {
                    return Ok(unified);
                }
                Err(parse_err) => {
                    let raw_text = serde_json::to_string(&raw).unwrap_or_default();
                    eprintln!("[WARN] LLM response parse failed (retry {}/{}): {}",
                        retry_count, max_retries, parse_err);
                    if retry_count >= max_retries {
                        eprintln!(
                            "[ERROR] ApiAdapter::call_llm_with_retry: \
                             parse failed after {} retries: {}. Raw response: {}",
                            max_retries,
                            parse_err,
                            serde_json::to_string(&raw).unwrap_or_default()
                        );
                        return Err(format!(
                            "Response parsing failed after {} retries: {}",
                            max_retries, parse_err
                        ));
                    }

                    let feature_pattern = match self.api_mode {
                        ApiMode::Chat => "tool_call",
                        ApiMode::Responses => "function_call",
                    };

                    if !raw_text.to_lowercase().contains(&feature_pattern.to_lowercase()) {
                        eprintln!(
                            "[ERROR] ApiAdapter::call_llm_with_retry: \
                             parse failed and no '{}' feature detected in response. Error: {}",
                            feature_pattern, parse_err
                        );
                        return Err(parse_err);
                    }

                    let retry_prompt = self.build_retry_prompt(&raw_text);
                    current_messages.push(serde_json::json!({
                        "role": "user",
                        "content": retry_prompt,
                    }));
                    retry_count += 1;
                }
            }
        }
    }

    /// Internal LLM invocation via HTTP (without tools).
    pub(crate) async fn invoke_llm(
        &self,
        messages: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        self.invoke_llm_with_tools(messages, &[], None).await
    }

    /// Internal LLM invocation via HTTP (with optional tools).
    pub(crate) async fn invoke_llm_with_tools(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        response_format: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        if self.api_key.is_empty() {
            return Err("LLM client not configured: api_key is empty".to_string());
        }

        let url = format!("{}/chat/completions", self.base_url);
        let api_key = self.api_key.clone();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "thinking": {
                "type": if self.thinking { "enabled" } else { "disabled" }
            },
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }
        if let Some(rf) = response_format {
            body["response_format"] = rf.clone();
        }

        let body_str = serde_json::to_string(&body)
            .map_err(|e| format!("Failed to serialize request body: {}", e))?;

        let timeout_dur = std::time::Duration::from_secs(60);
        let result = tokio::time::timeout(
            timeout_dur,
            tokio::task::spawn_blocking(move || {
                let response = minreq::post(&url)
                    .with_header("Authorization", format!("Bearer {}", api_key))
                    .with_header("Content-Type", "application/json")
                    .with_body(body_str)
                    .send()
                    .map_err(|e| format!("HTTP request failed: {}", e))?;

                let status = response.status_code;
                let body_str = response.as_str()
                    .map_err(|e| format!("Failed to read response body: {}", e))?;
                let response_body: serde_json::Value = serde_json::from_str(body_str)
                    .map_err(|e| format!("Failed to parse response: {}", e))?;

                if status != 200 {
                    let error_msg = response_body
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    return Err(format!("LLM API error ({}): {}", status, error_msg));
                }

                Ok(response_body)
            })
        ).await;

        match result {
            Ok(Ok(inner)) => inner,
            Ok(Err(join_err)) => Err(format!("Task join error: {}", join_err)),
            Err(_elapsed) => {
                eprintln!("[WARN] HTTP timeout after 60s, retrying...");
                Err("HTTP timeout after 60s".to_string())
            }
        }
    }
}
