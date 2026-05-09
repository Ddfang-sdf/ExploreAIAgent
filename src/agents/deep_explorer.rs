use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::adapter::api_adapter::LlmToolClient;
use crate::agents::exploration_refiner::ExplorationRefinerAgent;
use crate::agents::quality_evaluator::ExplorationQualityEvaluator;
use crate::agents::tool_result_refiner::ToolResultRefinerAgent;
use crate::common::config::DeepExplorerConfig;
use crate::context::exploration::{ExplorationContextTool, ExplorationSummary};
use crate::tools::registry::ToolRegistry;

pub const MAX_TOOL_CALLS: usize = 75;

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
    pub critical_files: Vec<super::search_strategy::CriticalFileRef>,
    pub collected_evidence: Vec<CollectedEvidence>,
    pub missing_info: String,
}

pub struct DeepExplorer {
    pub max_tool_calls: usize,
    loop_warning_threshold: usize,
    token_threshold: usize,
    token_target_ratio: f64,
    call_cache: HashSet<String>,
    consecutive_duplicates: usize,
}

/// Extract the first complete JSON object from text that may have non-JSON
/// prefix/suffix. Uses brace counting to handle nested objects and arrays.
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
            continue;
        }
        if b == b'\\' && in_string {
            escaped = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if b == b'{' || b == b'[' {
            depth += 1;
        } else if b == b'}' || b == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(text[start..=i].to_string());
            }
        }
    }
    None
}

impl DeepExplorer {
    pub fn new() -> Self {
        let defaults = DeepExplorerConfig::default();
        DeepExplorer {
            max_tool_calls: MAX_TOOL_CALLS,
            loop_warning_threshold: 3,
            token_threshold: defaults.token_threshold,
            token_target_ratio: defaults.token_target_ratio,
            call_cache: HashSet::new(),
            consecutive_duplicates: 0,
        }
    }

    pub fn from_config(config: &DeepExplorerConfig) -> Self {
        DeepExplorer {
            max_tool_calls: config.max_tool_calls,
            loop_warning_threshold: config.loop_warning_threshold,
            token_threshold: config.token_threshold,
            token_target_ratio: config.token_target_ratio,
            call_cache: HashSet::new(),
            consecutive_duplicates: 0,
        }
    }

    /// Returns the JSON Schema for the v1.2 response_format constraint (design doc 3.2.4).
    pub fn action_schema() -> serde_json::Value {
        serde_json::json!({
            "name": "deep_explorer_action",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["tool_call", "done"]},
                    "reasoning": {"type": "string"},
                    "tool": {"type": "string"},
                    "params": {"type": "object"},
                    "result": {
                        "type": "object",
                        "properties": {
                            "critical_files": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "path": {"type": "string"},
                                        "summary": {"type": "string"}
                                    },
                                    "required": ["path", "summary"],
                                    "additionalProperties": false
                                }
                            },
                            "collected_evidence": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "file": {"type": "string"},
                                        "line": {"type": "string"},
                                        "code_snippet": {"type": "string"},
                                        "relevance": {"type": "string"}
                                    },
                                    "required": ["file", "line", "code_snippet", "relevance"],
                                    "additionalProperties": false
                                }
                            },
                            "missing_info": {"type": "string"}
                        },
                        "required": ["critical_files", "collected_evidence", "missing_info"],
                        "additionalProperties": false
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }
        })
    }

    pub fn max_tool_calls(&self) -> usize {
        self.max_tool_calls
    }

    pub fn check_duplicate(&mut self, tool_name: &str, params_hash: &str) -> bool {
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

    pub fn generate_loop_warning(&self) -> Option<String> {
        if self.consecutive_duplicates >= self.loop_warning_threshold {
            Some(
                "## ⚠️ 系统警告\n你已连续多次执行完全相同的操作。请立即调整探索方向，尝试不同的工具、搜索词或文件路径。"
                    .to_string(),
            )
        } else {
            None
        }
    }

    pub fn assemble_prompt(
        &self,
        question: &str,
        current_summary: &ExplorationSummary,
    ) -> String {
        let template = String::from(
            "你是代码库深度探索专家。你的职责是基于已有的探索线索，深入代码库，自主调用底层只读工具，尽可能多地收集与用户问题相关的原始代码证据。\n\
             \n\
             {question}\n\
             {current_summary}\n\
             {loop_warning}\n\
             \n\
             ## 工作原则\n\
             \n\
             - **聚焦探索**：你的职责是深入代码库收集原始证据。系统会自动记录你的每次工具调用结果和关键发现，无需你手动记录。\n\
             - **避免短期重复**：不要在短时间内重复执行完全相同的操作（如同一文件的相同行范围、仅同义词替换的搜索）。\n\
             - **适时终止**：当收集到的证据足以回答用户问题时，**立即终止**。反复搜索同一个主题或在无关联的文件中遍历是浪费时间和资源的行为，必须禁止！如果你发现已经找到了关键代码或确认了用户问题的答案，不要再继续探索，直接输出 {\"action\": \"done\", ...}。\n\
             \n\
             ## 可用工具\n\
             \n\
             你可以使用以下六个只读工具，通过 JSON 告知系统调用：\n\
             - `search_content`：搜索文件内容，参数 pattern(正则), file_pattern(可选), exclude_paths(可选)\n\
             - `search_files`：按 glob 模式搜索文件名，参数 pattern(glob)\n\
             - `read_file`：读取文件内容，参数 file(路径), lines(行范围, 可选)\n\
             - `list_dir`：列出目录内容，参数 path(目录路径)\n\
             - `file_info`：获取文件元信息，参数 file(路径)\n\
             - `execute_shell`：执行只读 Shell 命令（如 find/grep/wc 等），参数 command。用于统计文件数量、查找特定类型文件、管道操作等结构化工具无法完成的任务\n\
             \n\
             ## 通信协议\n\
             \n\
             你的每次回复必须是合法的 JSON 对象，action 字段决定操作：\n\
             \n\
             **调用工具时**输出：\n\
             {\"action\": \"tool_call\", \"tool\": \"<工具名>\", \"params\": {<参数>}, \"reasoning\": \"<调用原因>\"}\n\
             \n\
             **终止探索时**输出：\n\
             {\"action\": \"done\", \"result\": {\"critical_files\": [...], \"collected_evidence\": [...], \"missing_info\": \"...\"}, \"reasoning\": \"<终止原因>\"}\n\
             \n\
             - `critical_files`：数组，列出探索过的最相关文件，每个元素为 {\"path\": \"文件路径\", \"summary\": \"一句话说明为什么相关\"}\n\
             - `collected_evidence`：数组，列举具体代码证据，每个元素为 {\"file\": \"路径\", \"line\": \"行号\", \"code_snippet\": \"代码片段\", \"relevance\": \"关联说明\"}\n\
             - `missing_info`：字符串，未找到的信息。如无，写\"无\"\n\
             \n\
             注意：你只需交付原始证据，无需对整体信息是否足够做出判断。"
        );

        let question_section = format!("## 用户问题\n{}", question);
        let prompt = template.replace("{question}", &question_section);

        let summary_section = {
            let json = serde_json::to_string(current_summary).unwrap_or_default();
            if json.is_empty() || json == "null" {
                "## 已有探索线索\n（无已有线索）".to_string()
            } else {
                format!("## 已有探索线索\n{}", json)
            }
        };
        let prompt = prompt.replace("{current_summary}", &summary_section);

        let warning = self.generate_loop_warning();
        let warning_section = match warning {
            Some(ref text) if !text.is_empty() => text.clone(),
            _ => String::new(),
        };
        prompt.replace("{loop_warning}", &warning_section)
    }

    pub async fn execute(
        &mut self,
        question: &str,
        current_summary: &ExplorationSummary,
        adapter: &dyn LlmToolClient,
        tool_registry: &ToolRegistry,
        ect: &ExplorationContextTool,
    ) -> Result<DeepExplorerResult, String> {
        let t0 = std::time::Instant::now();
        let prompt = self.assemble_prompt(question, current_summary);

        #[allow(unused_mut)]
        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "system", "content": prompt}),
        ];

        let mut tool_call_count: usize = 0;
        let mut parse_retries: usize = 0;
        let mut degradation_count: usize = 0;
        const MAX_PARSE_RETRIES: usize = 2;
        let max_context_tokens = self.token_threshold;
        const MAX_DEGRADATION: usize = 3;

        loop {
            // Per ExplorationRefinerAgent v1.1 Section 6.5:
            // Replace old raw truncation with Refiner-based semantic compression.
            let total_chars: usize = messages.iter()
                .map(|m| serde_json::to_string(m).unwrap_or_default().len())
                .sum();
            if total_chars / 4 > max_context_tokens {
                eprintln!("\r\x1b[K  \x1b[2m🗜️ 上下文过大，正在压缩...\x1b[0m");

                // Step 1: Read data from ECT
                let ect_summary = ect
                    .get_current_summary()
                    .unwrap_or(ExplorationSummary {
                        key_findings: String::new(),
                        critical_files: vec![],
                        missing_info: String::new(),
                        confidence: 0.0,
                    });
                let history = ect.get_history();
                let recent_records: Vec<_> = history
                    .into_iter()
                    .rev()
                    .take(15)
                    .collect();

                // Step 2: Calculate target_token_limit per Section 6.2
                let target_token_limit =
                    ((self.token_threshold as f64) * self.token_target_ratio).max(300.0) as usize;

                // Step 3: Call Refiner (adapter implements LlmStructuredClient via LlmToolClient)
                let refiner = ExplorationRefinerAgent::new();
                let refinement_result = refiner
                    .refine(
                        question,
                        &ect_summary,
                        &recent_records,
                        target_token_limit,
                        adapter,
                    )
                    .await;

                match refinement_result {
                    Ok(new_summary) => {
                        // Step 4: Write refined summary back to ECT
                        if let Err(_e) = ect.update_summary(new_summary) {
                            eprintln!("\r\x1b[K  \x1b[2m⚠️ 上下文压缩失败\x1b[0m");
                        }

                        // Step 5: Rebuild messages from ECT
                        let refined = ect
                            .get_current_summary()
                            .unwrap_or(ect_summary);
                        let new_prompt = self.assemble_prompt(question, &refined);
                        messages.clear();
                        messages.push(serde_json::json!({
                            "role": "system",
                            "content": new_prompt,
                        }));

                        // Keep last 2 raw tool result messages for LLM continuity
                        let history_after = ect.get_history();
                        let last_two: Vec<_> = history_after
                            .iter()
                            .rev()
                            .take(2)
                            .filter_map(|r| match r {
                                crate::context::exploration::ExplorationRecord::ToolCall {
                                    tool,
                                    result_summary,
                                    ..
                                } => {
                                    let fb = format!(
                                        "工具 {} 执行结果:\n{}",
                                        tool, result_summary
                                    );
                                    Some(serde_json::json!({
                                        "role": "user",
                                        "content": fb,
                                    }))
                                }
                                _ => None,
                            })
                            .collect();
                        for msg in last_two.into_iter().rev() {
                            messages.push(msg);
                        }

                        degradation_count = 0;
                        parse_retries = 0;
                        let new_chars: usize = messages.iter()
                            .map(|m| serde_json::to_string(m).unwrap_or_default().len())
                            .sum();
                        eprintln!("\r\x1b[K  \x1b[2m📦 压缩完成（{} → {} 字符）\x1b[0m", total_chars, new_chars);
                    }
                    Err(_refine_err) => {
                        // Section 6.5 Step 5: Degradation
                        degradation_count += 1;
                        eprintln!(
                            "\r\x1b[K  ⚠️ 压缩失败 ({}/{})，截断继续",
                            degradation_count, MAX_DEGRADATION
                        );

                        if degradation_count >= MAX_DEGRADATION {
                            eprintln!("\r\x1b[K  \x1b[2m⚠️ 压缩连续失败，终止探索\x1b[0m");
                            return Ok(DeepExplorerResult {
                                critical_files: vec![],
                                collected_evidence: vec![],
                                missing_info: "上下文精炼连续失败，探索被强制终止"
                                    .to_string(),
                            });
                        }

                        // Fallback: raw truncation, keep last 10 messages + system
                        let system_msg = messages.remove(0);
                        let keep = 10usize.min(messages.len());
                        let drained: Vec<_> = messages
                            .drain((messages.len() - keep)..)
                            .collect();
                        messages.clear();
                        messages.push(system_msg);
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": "[系统] 上下文已截断。请基于如下最近的探索结果继续，信息不足可以重新搜索。"
                        }));
                        messages.extend(drained);
                    }
                }
            }

            if tool_call_count >= self.max_tool_calls {
                return Ok(DeepExplorerResult {
                    critical_files: vec![],
                    collected_evidence: vec![],
                    missing_info: "探索达到上限被强制终止".to_string(),
                });
            }
            // Warn when approaching limit
            if tool_call_count >= self.max_tool_calls - 5 {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": "[系统] 探索次数即将达到上限，请尽快整理已收集的证据，输出 {\"action\": \"done\", ...} 终止探索。"
                }));
            }

            // Inject loop warning if triggered
            if let Some(warning) = self.generate_loop_warning() {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": warning,
                }));
            }

            // Send with json_object constraint to force valid JSON output
            let rf = serde_json::json!({"type": "json_object"});

            // ---- DEBUG: print token estimate ----
            {
                let total_chars: usize = messages.iter()
                    .map(|m| serde_json::to_string(m).unwrap_or_default().len())
                    .sum();
                let est_tokens = total_chars / 4;
                eprintln!("\r\x1b[K  \x1b[2mDE轮次{}: 消息数={} token≈{}\x1b[0m",
                    tool_call_count + 1, messages.len(), est_tokens);
            }

            let response = adapter
                .call_llm_with_tools(&messages, &[], Some(&rf))
                .await?;


            let text = match response.text {
                Some(t) if !t.is_empty() => t,
                _ => {
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": "你的回复为空。请按照 JSON 格式输出：{\"action\": \"tool_call\", ...} 或 {\"action\": \"done\", ...}"
                        }));
                        parse_retries += 1;
                        continue;
                    }
                    return Err("Empty response from LLM".to_string());
                }
            };

            // Extract JSON: LLM may wrap JSON in text. Find first '{' and match braces
            // to isolate the full JSON segment, handling nested objects/arrays.
            let json_text = match extract_json_object(&text) {
                Some(j) => j,
                None => {
                    // JSON parse retry — internal, not user-facing
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": "你的回复中未找到合法 JSON 对象。请只输出 JSON。",
                        }));
                        parse_retries += 1;
                        continue;
                    }
                    return Err("No JSON object found in response".to_string());
                }
            };
            let mut stream = serde_json::Deserializer::from_str(&json_text).into_iter::<serde_json::Value>();
            let mut parsed_any = false;
            loop {
                let action_json = match stream.next() {
                    Some(Ok(v)) => v,
                    Some(Err(e)) => {
                        if !parsed_any && parse_retries < MAX_PARSE_RETRIES {
                            // JSON parse retry — internal, not user-facing
                            messages.push(serde_json::json!({
                                "role": "user",
                                "content": format!("你的回复不是合法 JSON。请严格按照格式输出。错误: {}", e),
                            }));
                            parse_retries += 1;
                            break; // break inner, let outer loop retry
                        }
                        return Err(format!("JSON parse retry exhausted: {}", e));
                    }
                    None => break, // end of stream
                };
                parsed_any = true;
                parse_retries = 0;

                let action = action_json
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if action == "done" {
                    eprintln!("\r\x1b[K  ✓ 探索完成 ({} 轮)", tool_call_count + 1);
                } else {
                    let reasoning = action_json
                        .get("reasoning")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let text = if reasoning.is_empty() { "…".to_string() } else { reasoning.to_string() };
                    eprintln!("\r\x1b[K  \x1b[2m⬩ {}\x1b[0m", text);
                    crate::agents::main_agent::sse_send(
                        crate::agents::main_agent::SseEvent::Thinking(format!("⬩ 深度探索: {}", text))
                    );
                }

                match action {
                    "tool_call" => {
                        let tool_name = match action_json.get("tool").and_then(|v| v.as_str()) {
                            Some(n) => n.to_string(),
                            None => continue, // skip malformed entries in multi-response
                        };

                        let params = action_json
                            .get("params")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);

                        if tool_call_count >= self.max_tool_calls {
                            break;
                        }

                        let params_str = serde_json::to_string(&params).unwrap_or_default();
                        let is_dup = self.check_duplicate(&tool_name, &params_str);

                        let tool_result = if is_dup {
                            serde_json::json!({"cached": true, "message": "Duplicate call"})
                        } else {
                            tool_registry
                                .execute(&tool_name, params.clone())
                                .map(|output| output.data)
                                .unwrap_or_else(|e| {
                                    serde_json::json!({"success": false, "error": e.error})
                                })
                        };

                        // Auto-record to ECT (use parameter ect, not registry's internal ECT)
                        let _ = ect.write_record(
                            crate::context::exploration::ExplorationRecord::ToolCall {
                                source: "DeepExplorer".to_string(),
                                tool: tool_name.clone(),
                                params: params.clone(),
                                result_summary: serde_json::to_string(&tool_result).unwrap_or_default(),
                                confidence: 0.5,
                                timestamp: chrono::Utc::now(),
                            },
                        );

                        // v1.2: QE scoring per tool call — writes confidence to ECT only
                        let qe = ExplorationQualityEvaluator::new();
                        let _qe_result = qe.evaluate(question, &tool_result, adapter).await;
                        if let Ok(ref qe_summary) = _qe_result {
                            let _ = ect.write_record(
                                crate::context::exploration::ExplorationRecord::Summary {
                                    source: "ExplorationQualityEvaluator".to_string(),
                                    data: crate::context::exploration::ExplorationSummary {
                                        key_findings: qe_summary.key_findings.clone(),
                                        critical_files: vec![],
                                        missing_info: qe_summary.missing_info.clone(),
                                        confidence: qe_summary.confidence,
                                    },
                                    confidence: qe_summary.confidence,
                                    timestamp: chrono::Utc::now(),
                                },
                            );
                        }

                        // v1.3: TR skip threshold — if raw result fits in remaining
                        // token budget (1000 total), skip TR to avoid LLM cost.
                        let current_chars: usize = messages.iter()
                            .map(|m| serde_json::to_string(m).unwrap_or_default().len())
                            .sum();
                        let raw_json = serde_json::to_string(&tool_result).unwrap_or_default();
                        let raw_tokens = raw_json.len() / 4;
                        let current_tokens = current_chars / 4;
                        const TR_SKIP_BUDGET: usize = 1000;

                        let result_feedback = if current_tokens + raw_tokens <= TR_SKIP_BUDGET {
                            // Tool result is small enough — skip TR, use raw JSON directly
                            format!("工具 {} 执行结果:\n{}", tool_name, raw_json)
                        } else {
                            // Tool result exceeds budget — use TR to refine
                            let tr = ToolResultRefinerAgent::new();
                            let tr_result = tr.refine(question, &tool_name, &tool_result, adapter).await;
                            match &tr_result {
                                Ok(refined) => {
                                    format!("工具 {} 结果: {}", tool_name, refined.summary)
                                }
                                Err(_) => {
                                    let truncated: String = raw_json.chars().take(500).collect();
                                    format!("工具 {} 执行结果 (截断):\n{}...", tool_name, truncated)
                                }
                            }
                        };
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": result_feedback,
                        }));

                        // Trim old tool results: ECT holds full history,
                        // LLM only needs last 2 rounds for continuity.
                        if messages.len() > 5 {
                            let system = messages.remove(0);
                            let keep_start = messages.len().saturating_sub(4);
                            let kept: Vec<_> = messages.drain(keep_start..).collect();
                            messages.clear();
                            messages.push(system);
                            messages.extend(kept);
                        }

                        tool_call_count += 1;
                    }

                    "done" => {
                        let result_value = action_json
                            .get("result")
                            .ok_or("action=done missing 'result' field")?;
                        let result: DeepExplorerResult = serde_json::from_value(result_value.clone())
                            .map_err(|e| format!("Failed to parse exploration result: {}", e))?;
                        eprintln!("\r\x1b[K  \x1b[2mDE完成：{} 次工具调用，耗时 {:.1}s\x1b[0m", tool_call_count, t0.elapsed().as_secs_f64());
                        return Ok(result);
                    }

                    _ => {} // skip unknown actions in stream
                }
            }
        }
    }
}
