use crate::adapter::api_adapter::LlmStructuredClient;

pub struct MainAgent;

impl MainAgent {
    pub fn new() -> Self {
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
