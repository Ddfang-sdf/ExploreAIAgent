use super::api_adapter::ApiAdapter;
use super::types::{ApiMode, UnifiedResponse};

// OpenCode-style retry constants
pub(crate) const MAX_HTTP_RETRIES: usize = 2;
pub(crate) const BASE_DELAY_MS: u64 = 500;
pub(crate) const MAX_DELAY_MS: u64 = 10_000;

pub(crate) fn retryable_status(status: i32) -> bool {
    status == 429 || status == 503 || status == 504 || status == 529
}

/// Extract retry-after delay (ms) from a `[retry_after=N]` prefix in the error message.
pub(crate) fn parse_error_retry_after(error: &str) -> Option<u64> {
    if let Some(rest) = error.strip_prefix("[retry_after=") {
        if let Some(end) = rest.find(']') {
            return rest[..end].parse::<u64>().ok();
        }
    }
    None
}

/// Strip the `[retry_after=N]` prefix from an error message for display.
pub(crate) fn strip_retry_prefix(error: &str) -> &str {
    if error.starts_with("[retry_after=") {
        if let Some(end) = error.find("] ") {
            return &error[end + 2..];
        }
    }
    error
}

/// Simple pseudo-random jitter factor: 0.8 ~ 1.2
pub(crate) fn jitter() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    0.8 + (ns as f64 % 0.4)
}

pub(crate) fn retry_delay(attempt: usize, retry_after_ms: Option<u64>) -> std::time::Duration {
    if let Some(ms) = retry_after_ms {
        return std::time::Duration::from_millis(ms.min(MAX_DELAY_MS));
    }
    let base = BASE_DELAY_MS * (1u64 << attempt);
    let j = jitter();
    let ms = ((base as f64 * j) as u64).min(MAX_DELAY_MS);
    std::time::Duration::from_millis(ms)
}

impl ApiAdapter {

    /// Parse `retry-after-ms` / `retry-after` from HTTP response headers.
    fn parse_retry_after(resp: &minreq::Response) -> Option<u64> {
        let iter: Vec<(String, String)> = resp.headers.iter()
            .map(|(k, v)| (k.to_lowercase(), v.to_string()))
            .collect();
        // retry-after-ms (preferred)
        for (name, value) in &iter {
            if name == "retry-after-ms" {
                if let Ok(ms) = value.parse::<u64>() {
                    return Some(ms.min(MAX_DELAY_MS));
                }
            }
        }
        // retry-after (seconds)
        for (name, value) in &iter {
            if name == "retry-after" {
                if let Ok(secs) = value.parse::<u64>() {
                    return Some((secs * 1000).min(MAX_DELAY_MS));
                }
            }
        }
        None
    }
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
        let mut parse_fail_count: usize = 0;
        const MAX_PARSE_FAILS: usize = 2;

        loop {
            let raw_response = self.invoke_llm_with_tools(&current_messages, tools, response_format).await;
            let raw = match &raw_response {
                Ok(r) => r.clone(),
                Err(e) => {
                    let retry_after = parse_error_retry_after(&e);
                    if retry_count < MAX_HTTP_RETRIES && retry_after.is_some() {
                        // Rate limit: use server-specified delay
                        retry_count += 1;
                        let delay = retry_delay(retry_count - 1, retry_after);
                        let msgs = current_messages.len();
                        eprintln!("[WARN] rate-limit, retry {}/{} after {}ms ({} msgs)", retry_count, MAX_HTTP_RETRIES, delay.as_millis(), msgs);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    if retry_count < MAX_HTTP_RETRIES {
                        // Other transient error: exponential + jitter
                        retry_count += 1;
                        let delay = retry_delay(retry_count - 1, None);
                        let msgs = current_messages.len();
                        eprintln!("[WARN] transient, retry {}/{} after {}ms ({} msgs)", retry_count, MAX_HTTP_RETRIES, delay.as_millis(), msgs);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(format!("LLM call failed after {} retries: {}", MAX_HTTP_RETRIES, strip_retry_prefix(&e)));
                }
            };

            let parse_result = self.parse_response(&raw);

            match parse_result {
                Ok(unified) => {
                    return Ok(unified);
                }
                Err(parse_err) => {
                    let raw_text = serde_json::to_string(&raw).unwrap_or_default();
                    eprintln!("[WARN] LLM response parse failed (parse retry {}/{}): {}",
                        parse_fail_count, MAX_PARSE_FAILS, parse_err);
                    if parse_fail_count >= MAX_PARSE_FAILS {
                        eprintln!(
                            "[ERROR] ApiAdapter::call_llm_with_retry: \
                             parse failed after {} retries: {}. Raw response: {}",
                            MAX_PARSE_FAILS,
                            parse_err,
                            serde_json::to_string(&raw).unwrap_or_default()
                        );
                        return Err(format!(
                            "Response parsing failed after {} retries: {}",
                            MAX_PARSE_FAILS, parse_err
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
                    parse_fail_count += 1;
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

        let api_path = self.model_adapter.as_ref().map(|a| a.api_path()).unwrap_or("/chat/completions");
        let url = format!("{}{}", self.base_url, api_path);
        let api_key = self.api_key.clone();

        let body = if let Some(ref adapter) = self.model_adapter {
            adapter.build_request_body(&self.model, messages, tools, response_format)
        } else {
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
            body
        };

        let body_str = serde_json::to_string(&body)
            .map_err(|e| format!("Failed to serialize request body: {}", e))?;

        let msg_count = messages.len();
        let est_tokens = body_str.len() / 4;

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
                    // Diagnostic: dump ALL response headers on non-200
                    let headers: Vec<String> = response.headers.iter()
                        .map(|(k, v)| format!("  {}: {}", k, v))
                        .collect();
                    let body_preview: String = body_str.chars().take(300).collect();
                    eprintln!("[HTTP {}] response headers:\n{}\n  body(preview): {}", status, headers.join("\n"), body_preview);
                    // Parse retry-after header (OpenCode-style)
                    let retry_after_ms = Self::parse_retry_after(&response);
                    let error_msg = response_body
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    let prefix = if let Some(ms) = retry_after_ms {
                        format!("[retry_after={}] ", ms)
                    } else {
                        String::new()
                    };
                    return Err(format!("{}LLM API error ({}): {}", prefix, status, error_msg));
                }

                Ok(response_body)
            })
        ).await;

        match result {
            Ok(Ok(inner)) => inner,
            Ok(Err(join_err)) => Err(format!("Task join error: {}", join_err)),
            Err(_elapsed) => {
                let purpose_label = if msg_count <= 1 { "compact" } else { "main" };
                eprintln!("[WARN:{}] HTTP timeout after 60s ({} msgs, ~{} tok)", purpose_label, msg_count, est_tokens);
                Err("HTTP timeout after 60s".to_string())
            }
        }
    }

    /// Streaming LLM call via SSE. Calls `on_reasoning` for each reasoning delta
    /// as it arrives, then returns the accumulated (raw_json, UnifiedResponse).
    pub(crate) async fn invoke_llm_streaming(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        response_format: Option<&serde_json::Value>,
        on_reasoning: impl Fn(&str) + Send + 'static,
    ) -> Result<(serde_json::Value, super::types::UnifiedResponse), String> {
        if self.api_key.is_empty() {
            return Err("LLM client not configured: api_key is empty".to_string());
        }

        let api_path = self.model_adapter.as_ref().map(|a| a.api_path()).unwrap_or("/chat/completions");
        let url = format!("{}{}", self.base_url, api_path);
        let api_key = self.api_key.clone();

        let mut body = if let Some(ref adapter) = self.model_adapter {
            adapter.build_request_body(&self.model, messages, tools, response_format)
        } else {
            let mut b = serde_json::json!({
                "model": self.model,
                "messages": messages,
            });
            if !tools.is_empty() { b["tools"] = serde_json::json!(tools); }
            if let Some(rf) = response_format { b["response_format"] = rf.clone(); }
            b
        };
        body["stream"] = serde_json::json!(true);

        let body_str = serde_json::to_string(&body)
            .map_err(|e| format!("Failed to serialize request body: {}", e))?;

        let url_clone = url.clone();
        let api_key_clone = api_key.clone();
        let body_str_clone = body_str.clone();

        let (raw_json, unified) = tokio::task::spawn_blocking(move || {
            let response = minreq::post(&url_clone)
                .with_header("Authorization", format!("Bearer {}", api_key_clone))
                .with_header("Content-Type", "application/json")
                .with_body(body_str_clone)
                .send()
                .map_err(|e| format!("HTTP request failed: {}", e))?;

            let status = response.status_code;
            let body_str = response.as_str()
                .map_err(|e| format!("Failed to read response body: {}", e))?;
            if status != 200 {
                let preview: String = body_str.chars().take(300).collect();
                eprintln!("[HTTP {}] body: {}", status, preview);
                return Err(format!("LLM API error ({}): {}", status, preview));
            }

            // Detect SSE format: OpenAI (choices[]) or Anthropic (content_block)
            let is_anthropic = body_str.contains("\"content_block_start\"") || body_str.contains("\"content_block_delta\"");

            let mut full_content = String::new();
            let mut tool_calls_map: std::collections::BTreeMap<i32, (String, String, String)> = std::collections::BTreeMap::new();
            let mut reasoning_parts: Vec<String> = Vec::new();
            let mut finish_reason: Option<String> = None;

            if is_anthropic {
                // --- Anthropic SSE format ---
                let mut current_event: Option<String> = None;
                for line in body_str.lines() {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    if line.starts_with("event: ") {
                        current_event = Some(line[7..].to_string());
                        continue;
                    }
                    if !line.starts_with("data: ") { continue; }
                    let json_str = &line[6..];
                    let ev: serde_json::Value = serde_json::from_str(json_str)
                        .map_err(|e| format!("SSE parse error: {} in: {}", e, &json_str[..200.min(json_str.len())]))?;

                    let ev_type = ev["type"].as_str().unwrap_or("");
                    match ev_type {
                        "content_block_start" => {
                            let cb = &ev["content_block"];
                            let idx = ev["index"].as_i64().unwrap_or(0) as i32;
                            match cb["type"].as_str().unwrap_or("") {
                                "thinking" => {
                                    if let Some(t) = cb["thinking"].as_str() {
                                        on_reasoning(t);
                                        reasoning_parts.push(t.to_string());
                                    }
                                }
                                "tool_use" => {
                                    let id = cb["id"].as_str().unwrap_or("").to_string();
                                    let name = cb["name"].as_str().unwrap_or("").to_string();
                                    let input_str = cb["input"].as_object()
                                        .filter(|o| !o.is_empty())
                                        .map(|o| serde_json::to_string(o).unwrap_or_default())
                                        .unwrap_or_default();
                                    tool_calls_map.entry(idx).or_insert_with(|| (id, name, input_str));
                                }
                                _ => {}
                            }
                        }
                        "content_block_delta" => {
                            let delta = &ev["delta"];
                            let idx = ev["index"].as_i64().unwrap_or(0) as i32;
                            match delta["type"].as_str().unwrap_or("") {
                                "thinking_delta" => {
                                    if let Some(t) = delta["thinking"].as_str() {
                                        on_reasoning(t);
                                        reasoning_parts.push(t.to_string());
                                    }
                                }
                                "text_delta" => {
                                    if let Some(t) = delta["text"].as_str() {
                                        full_content.push_str(t);
                                    }
                                }
                                "input_json_delta" => {
                                    if let Some(entry) = tool_calls_map.get_mut(&idx) {
                                        if let Some(j) = delta["partial_json"].as_str() {
                                            entry.2.push_str(j);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        "message_delta" => {
                            if let Some(sr) = ev["delta"]["stop_reason"].as_str() {
                                finish_reason = Some(sr.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                // --- OpenAI SSE format ---
                for line in body_str.lines() {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    if line == "data: [DONE]" { break; }
                    if !line.starts_with("data: ") { continue; }

                    let json_str = &line[6..];
                    let event: serde_json::Value = serde_json::from_str(json_str)
                        .map_err(|e| format!("SSE JSON parse error: {} in: {}", e, &json_str[..200.min(json_str.len())]))?;

                    if let Some(fr) = event["choices"][0]["finish_reason"].as_str() {
                        finish_reason = Some(fr.to_string());
                    }

                    let delta = &event["choices"][0]["delta"];

                    if let Some(rd) = delta["reasoning_details"].as_array() {
                        for item in rd {
                            if item["type"] == "reasoning.text" {
                                if let Some(text) = item["text"].as_str() {
                                    on_reasoning(text);
                                    reasoning_parts.push(text.to_string());
                                }
                            }
                        }
                    }
                    for key in &["reasoning", "reasoning_text"] {
                        if let Some(reason) = delta[*key].as_str() {
                            if !reason.is_empty() {
                                on_reasoning(reason);
                                reasoning_parts.push(reason.to_string());
                            }
                        }
                    }
                    if let Some(content) = delta["content"].as_str() {
                        if !content.is_empty() {
                            let mut remaining = content;
                            loop {
                                if let Some(start) = remaining.find("<think>") {
                                    if start > 0 { full_content.push_str(&remaining[..start]); }
                                    let after_open = &remaining[start + 7..];
                                    if let Some(end) = after_open.find("</think>") {
                                        let think_text = &after_open[..end];
                                        if !think_text.is_empty() {
                                            on_reasoning(think_text);
                                            reasoning_parts.push(think_text.to_string());
                                        }
                                        remaining = &after_open[end + 8..];
                                    } else {
                                        let think_text = after_open;
                                        if !think_text.is_empty() {
                                            on_reasoning(think_text);
                                            reasoning_parts.push(think_text.to_string());
                                        }
                                        break;
                                    }
                                } else {
                                    full_content.push_str(remaining);
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(tc_array) = delta["tool_calls"].as_array() {
                        for tc in tc_array {
                            let idx = tc["index"].as_i64().unwrap_or(0) as i32;
                            let entry = tool_calls_map.entry(idx).or_insert_with(|| (String::new(), String::new(), String::new()));
                            if let Some(id) = tc["id"].as_str() { entry.0 = id.to_string(); }
                            if let Some(func) = tc.get("function") {
                                if let Some(name) = func["name"].as_str() { entry.1 = name.to_string(); }
                                if let Some(args) = func["arguments"].as_str() { entry.2.push_str(args); }
                            }
                        }
                    }
                }
            }

            // Build tool_calls from accumulated deltas
            let mut tool_calls = Vec::new();
            for (_idx, (id, name, args)) in tool_calls_map {
                let arguments: serde_json::Value = if args.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_str(&args).unwrap_or(serde_json::Value::Null)
                };
                tool_calls.push(super::types::ToolCallInfo {
                    id: Some(id),
                    name,
                    arguments,
                });
            }

            let text = if full_content.is_empty() || full_content == "\n" {
                None
            } else {
                Some(full_content)
            };

            let reasoning = if reasoning_parts.is_empty() { None } else { Some(reasoning_parts.join("")) };

            // Build a synthetic raw response for build_assistant_message
            let raw_json = serde_json::json!({
                "choices": [{
                    "finish_reason": finish_reason.unwrap_or_default(),
                    "message": {
                        "role": "assistant",
                        "content": text.clone().unwrap_or_default(),
                        "tool_calls": tool_calls.iter().map(|tc| {
                            serde_json::json!({
                                "id": tc.id.clone().unwrap_or_default(),
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                                }
                            })
                        }).collect::<Vec<_>>(),
                    }
                }]
            });

            let unified = super::types::UnifiedResponse { text, tool_calls, reasoning };
            Ok::<(serde_json::Value, super::types::UnifiedResponse), String>((raw_json, unified))
        }).await
        .map_err(|e| format!("Task join error: {}", e))?
        ?;

        Ok((raw_json, unified))
    }
}
