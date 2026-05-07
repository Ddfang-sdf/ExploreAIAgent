use serde::{Deserialize, Serialize};

use crate::adapter::api_adapter::LlmStructuredClient;

/// A single round of conversation history for the refiner.
/// Matches design doc section 4.5 {recent_conversation_history} format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRoundRecord {
    pub round: u32,
    pub user_question: String,
    pub answer_summary: String,
    pub topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRefinerOutput {
    pub summary: String,
}

pub const CONVERSATION_REFINER_SCHEMA: &str = r#"{
  "name": "conversation_refiner_response",
  "strict": true,
  "schema": {
    "type": "object",
    "properties": {
      "summary": {
        "type": "string",
        "description": "压缩后的对话摘要"
      }
    },
    "required": ["summary"],
    "additionalProperties": false
  }
}"#;

pub struct ConversationRefinerAgent;

impl ConversationRefinerAgent {
    pub fn new() -> Self {
        ConversationRefinerAgent
    }

    pub async fn refine(
        &self,
        user_question: &str,
        recent_conversation_history: &[ConversationRoundRecord],
        existing_summary: &str,
        client: &dyn LlmStructuredClient,
    ) -> Result<ConversationRefinerOutput, String> {
        // Step 1: construct input_data
        let input_data = serde_json::json!({
            "user_question": user_question,
            "recent_conversation_history": recent_conversation_history,
            "existing_summary": existing_summary,
        });

        // Step 2: assemble instruction text
        let instructions = Self::assemble_prompt();

        // Step 3: call adapter with schema constraint
        let schema_value: serde_json::Value =
            serde_json::from_str(Self::output_schema()).map_err(|e| {
                format!("Failed to parse output schema JSON: {}", e)
            })?;

        let response = client
            .call_llm_structured(&instructions, &input_data, Some(&schema_value))
            .await?;

        // Step 4: parse response
        let text = match response.text {
            Some(t) if !t.is_empty() => t,
            _ => return Err("Empty response from LLM".to_string()),
        };

        let output: ConversationRefinerOutput = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse refinement JSON: {}", e))?;

        Ok(output)
    }

    pub fn output_schema() -> &'static str {
        CONVERSATION_REFINER_SCHEMA
    }

    pub fn assemble_prompt() -> String {
        String::from(
            "你是对话上下文精炼专家。将完整对话历史压缩为极简摘要，保留关键话题脉络和重要指代关系。\n\
             \n\
             系统会以结构化数据的形式向你提供用户当前问题、最近对话记录和已有历史摘要，请基于这些内容完成精炼。\n\
             \n\
             ## 精炼要求\n\
             1. **保留话题演变**：清晰描述用户先后讨论了哪些话题，以及话题之间的关联。\n\
             2. **保留指代关系**：明确指出当前问题中的指代词（如\"它\"、\"这个参数\"）具体指代前文中的哪个概念或实体。\n\
             3. **去除冗余**：删除寒暄、重复确认、与话题无关的闲聊。\n\
             4. **长度控制**：输出摘要的总 Token 数必须控制在 500 以内。\n\
             \n\
             ## 输出格式（强制约束）\n\
             你必须**只输出一个合法的 JSON 对象**，不要包裹任何标记、不要添加任何解释文字。\n\
             \n\
             - `summary`：字符串，压缩后的对话摘要。\n\
             \n\
             **示例输出**：\n\
             {\n\
               \"summary\": \"第1-2轮讨论了 BooleanValidator 的基本用法，第3-4轮追问了它的参数配置。当前问题中的'它'指代 BooleanValidator，用户想了解 required 参数的默认值。\"\n\
             }\n\
             \n\
             **警告**：如果你输出的不是合法 JSON，或者缺少 `summary` 字段，系统将拒绝你的输出并要求你重新生成。",
        )
    }
}
