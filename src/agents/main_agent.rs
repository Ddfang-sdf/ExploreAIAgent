use crate::adapter::api_adapter::{LlmStructuredClient, LlmToolClient};

// ============================================================================
// v1.2: Trait definitions for dependency injection
// ============================================================================

#[async_trait::async_trait]
pub trait FastExploreExecutor: Send + Sync {
    async fn execute(&self, keywords: &[String]) -> Result<serde_json::Value, String>;
}

#[async_trait::async_trait]
pub trait DeepExploreExecutor: Send + Sync {
    async fn execute(
        &self,
        question: &str,
        current_summary: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String>;
}

pub struct MainAgent;

impl MainAgent {
    pub fn new() -> Self {
        MainAgent
    }

    /// v1.2: Decision loop — JSON communication with LLM, tool dispatch
    pub async fn run(
        &self,
        question: &str,
        conversation_context: &str,
        fast_explore: &dyn FastExploreExecutor,
        de: &dyn DeepExploreExecutor,
        client: &dyn LlmToolClient,
    ) -> Result<String, String> {
        let system_prompt = Self::assemble_prompt();
        let user_content = if conversation_context.is_empty() {
            question.to_string()
        } else {
            format!("{}\n{}", conversation_context, question)
        };

        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": user_content}),
        ];

        let schema = Self::action_schema();
        let mut parse_retries: usize = 0;
        const MAX_PARSE_RETRIES: usize = 2;
        let mut first_call = true;

        loop {
            if first_call {
                eprintln!("⏳ 正在分析问题...");
                first_call = false;
            }
            let rf = serde_json::json!({"type": "json_object"});
            let response = client
                .call_llm_with_tools(&messages, &[], Some(&rf))
                .await?;

            let text = match response.text {
                Some(t) if !t.is_empty() => t,
                _ => {
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": "你的回复为空。请按照 JSON 格式输出：{\"action\": \"answer\", ...} 或 {\"action\": \"tool_call\", ...}"
                        }));
                        parse_retries += 1;
                        continue;
                    }
                    return Err("Empty response from LLM".to_string());
                }
            };

            // Parse JSON action
            let action_json: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": format!("你的回复不是合法 JSON。请严格按照格式输出。错误: {}", e),
                        }));
                        parse_retries += 1;
                        continue;
                    }
                    return Err(format!("JSON parse retry exhausted: {}", e));
                }
            };
            parse_retries = 0;

            let action = action_json.get("action").and_then(|v| v.as_str()).unwrap_or("");

            match action {
                "answer" => {
                    let final_response = action_json
                        .get("final_response")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    return Ok(final_response.to_string());
                }
                "tool_call" => {
                    let tool = action_json.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                    match tool {
                        "fast_explore" => {
                            eprintln!("\r\x1b[K🔍 正在搜索代码库...");
                            let keywords: Vec<String> = action_json
                                .get("arguments")
                                .and_then(|a| a.get("keywords"))
                                .and_then(|k| k.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default();

                            match fast_explore.execute(&keywords).await {
                                Ok(result) => {
                                    let result_str = serde_json::to_string(&result).unwrap_or_default();
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("fast_explore 返回结果:\n{}", result_str),
                                    }));
                                }
                                Err(e) => {
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("fast_explore 执行失败: {}", e),
                                    }));
                                }
                            }
                        }
                        "deep_explore" => {
                            eprintln!("\r\x1b[K🔍 正在深入探索代码...");
                            let de_question = action_json
                                .get("arguments")
                                .and_then(|a| a.get("question"))
                                .and_then(|q| q.as_str())
                                .unwrap_or(question);
                            let summary = action_json.get("arguments").and_then(|a| a.get("current_summary"));

                            match de.execute(de_question, summary).await {
                                Ok(result) => {
                                    let result_str = serde_json::to_string(&result).unwrap_or_default();
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("deep_explore 返回结果:\n{}", result_str),
                                    }));
                                }
                                Err(e) => {
                                    messages.push(serde_json::json!({
                                        "role": "user",
                                        "content": format!("deep_explore 执行失败: {}", e),
                                    }));
                                }
                            }
                        }
                        _ => {
                            messages.push(serde_json::json!({
                                "role": "user",
                                "content": "未知工具。可用工具为 fast_explore 和 deep_explore。",
                            }));
                        }
                    }
                }
                _ => {
                    if parse_retries < MAX_PARSE_RETRIES {
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": "缺少 action 字段。请输出 {\"action\": \"answer\", ...} 或 {\"action\": \"tool_call\", ...}",
                        }));
                        parse_retries += 1;
                    } else {
                        return Err("Missing action field after retries".to_string());
                    }
                }
            }
        }
    }

    pub fn action_schema() -> serde_json::Value {
        serde_json::json!({
            "name": "main_agent_action",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["answer", "tool_call"]},
                    "final_response": {"type": "string"},
                    "tool": {"type": "string", "enum": ["fast_explore", "deep_explore"]},
                    "arguments": {"type": "object"}
                },
                "required": ["action"]
            }
        })
    }

    #[deprecated]
    pub fn new_legacy() -> Self {
        MainAgent
    }

    /// Extract the answer from LLM response text.
    /// Tries in order: <final_response> tags, JSON "final_response" field, raw text.
    fn extract_answer(raw: &str) -> String {
        // Try <final_response>...</final_response> tags
        if let Some(start) = raw.find("<final_response>") {
            let after_tag = &raw[start + "<final_response>".len()..];
            if let Some(end) = after_tag.find("</final_response>") {
                return after_tag[..end].trim().to_string();
            }
        }

        // Try JSON: parse and look for "final_response" key
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(text) = json.get("final_response").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }

        raw.trim().to_string()
    }

    pub async fn generate_answer(
        &self,
        user_question: &str,
        conversation_context: &str,
        exploration_data: &serde_json::Value,
        client: &dyn LlmStructuredClient,
    ) -> Result<String, String> {
        let input_data = serde_json::json!({
            "user_question": user_question,
            "conversation_context": conversation_context,
            "exploration_data": exploration_data,
        });

        let instructions = Self::assemble_prompt();

        let response = client
            .call_llm_structured(&instructions, &input_data, None)
            .await?;

        match response.text {
            Some(text) if !text.is_empty() => Ok(Self::extract_answer(&text)),
            _ => Err("Empty response from LLM".to_string()),
        }
    }

    pub fn assemble_prompt() -> String {
        String::from(
            "你是探索者（Explore AI Agent），一个专业的代码库探索助手。\n\
             你的工作方式是：理解用户问题，必要时调用代码库搜索工具获取信息，基于搜索结果回答用户。\n\
             \n\
             ## 可用工具\n\
             \n\
             你可以调用以下两个工具。工具的具体输入输出格式由系统控制，以下是它们的能力描述和数据结构：\n\
             \n\
             ### fast_explore — 快速扫描代码库\n\
             \n\
             根据你设计的关键词批量搜索代码库，返回线索摘要和置信度评分。\n\
             \n\
             | 项目 | 说明 |\n\
             |:---|:---|\n\
             | 输入 | keywords（字符串数组）：2-5 个搜索关键词。你需要自己设计关键词——从用户问题中提取核心概念，中英文兼顾 |\n\
             | 输出 | matches（搜索结果）、key_findings（核心发现）、critical_files（关键文件列表）、confidence（置信度 0.0~1.0） |\n\
             | 适用 | 首次探索、快速了解项目中有哪些相关模块、不确定方向时 |\n\
             | 限制 | 单次扫描，只返回概要。如果结果不理想，可以调整关键词后再次调用 |\n\
             \n\
             输出示例：\n\
             {\"matches\": [...], \"key_findings\": \"回测模块在 backtest/engine.py 中实现\", \"critical_files\": [{\"path\": \"src/backtest/engine.py\", \"summary\": \"回测引擎核心\"}], \"confidence\": 0.8}\n\
             \n\
             ### deep_explore — 深度代码探索\n\
             \n\
             深入阅读代码文件，精确定位代码证据。\n\
             \n\
             | 项目 | 说明 |\n\
             |:---|:---|\n\
             | 输入 | question（字符串）：要调查的问题。current_summary（对象，可选）：已有的探索线索摘要 |\n\
             | 输出 | critical_files（相关文件及说明）、collected_evidence（代码证据列表，每条含 file、line、code_snippet、relevance）、missing_info（缺失信息） |\n\
             | 适用 | fast_explore 指出关键文件但缺少细节、需要确认代码逻辑、追溯调用链 |\n\
             | 限制 | 耗时较长（内部最多 75 次操作）。通常先 fast_explore 再 deep_explore |\n\
             \n\
             输出示例：\n\
             {\"critical_files\": [{\"path\": \"src/backtest/engine.py\", \"summary\": \"回测引擎核心\"}], \"collected_evidence\": [{\"file\": \"src/backtest/engine.py\", \"line\": \"142-158\", \"code_snippet\": \"def run_backtest...\", \"relevance\": \"回测主循环\"}], \"missing_info\": \"无\"}\n\
             \n\
             ## 决策规则（严格遵守）\n\
             \n\
             1. 任何关于代码库的问题，必须先调 fast_explore 获取数据，再回答。严禁在未探索的情况下猜测或说\"信息不足\"。\n\
             2. fast_explore 返回线索后：信息不够 → 调 deep_explore 深入；信息够了 → 直接回答。\n\
             3. 只有以下情况可以不调工具直接回答：\n\
                - 纯问候（\"你好\"、\"谢谢\"、\"再见\"）\n\
                - 追问刚才已探索过的话题（\"再详细说说\"）\n\
             4. 严禁编造代码细节。探索之后数据仍不够，才可以说\"探索后未找到相关信息\"。\n\
             \n\
             ## 通信协议\n\
             \n\
             你与系统之间通过 JSON 通信。每次回复必须是合法的 JSON 对象，action 字段决定操作类型：\n\
             \n\
             直接回答用户时：\n\
             {\"action\": \"answer\", \"final_response\": \"答案内容\"}\n\
             \n\
             调用工具时：\n\
             {\"action\": \"tool_call\", \"tool\": \"fast_explore\", \"arguments\": {\"keywords\": [\"回测\", \"backtest\", \"引擎\"]}}\n\
             {\"action\": \"tool_call\", \"tool\": \"deep_explore\", \"arguments\": {\"question\": \"用户问题\", \"current_summary\": {...}}}\n\
             \n\
             注意：只输出 JSON，不要包裹任何标记或解释文字。如果你的回答不符合 JSON 要求，系统将强制你重新回答，请务必按照通信协议规范的返回 JSON！",
        )
    }

    /// v1.1 方法（保留兼容，待清理）
    fn assemble_prompt_legacy() -> String {
        String::from(
            "你是探索者（Explore AI Agent），一个专业的代码库探索助手。基于系统提供的探索数据回答用户问题。\n\
             \n\
             系统会以结构化数据的形式向你提供对话上下文、用户问题和探索数据，请基于这些内容生成答案。\n\
             \n\
             ## 要求\n\
             - 仅基于提供的探索数据回答，不要凭空编造\n\
             - 如果探索数据不足以回答问题，如实告知用户\n\
             - 回答专业、准确、简洁\n\
             - 结合对话上下文理解多轮对话中的指代关系\n\
             \n\
             ## 输出格式\n\
             直接输出答案，用 <final_response> 标签包裹。\n\
             \n\
             例如：\n\
             <final_response>\n\
             BooleanValidator 支持两个配置参数：required（默认 true）和 defaultValue。required 参数控制……\n\
             </final_response>",
        )
    }
}
