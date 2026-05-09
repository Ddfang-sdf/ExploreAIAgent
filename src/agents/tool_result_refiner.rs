use serde::{Deserialize, Serialize};

use crate::adapter::api_adapter::LlmStructuredClient;

/// Output of ToolResultRefinerAgent — refined single tool-call result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinedToolResult {
    pub summary: String,
}

/// JSON Schema for structured output constraint (TR design doc 2.4).
pub const TOOL_RESULT_REFINER_SCHEMA: &str = r#"{
  "name": "tool_result_refined",
  "strict": true,
  "schema": {
    "type": "object",
    "properties": {
      "summary": {
        "type": "string",
        "description": "提炼后的可操作线索，包含文件路径、关键符号名、代码片段、匹配统计等"
      }
    },
    "required": ["summary"],
    "additionalProperties": false
  }
}"#;

pub struct ToolResultRefinerAgent;

impl ToolResultRefinerAgent {
    pub fn new() -> Self {
        ToolResultRefinerAgent
    }

    pub async fn refine(
        &self,
        question: &str,
        tool_name: &str,
        tool_result: &serde_json::Value,
        client: &dyn LlmStructuredClient,
    ) -> Result<RefinedToolResult, String> {
        let input_data = serde_json::json!({
            "question": question,
            "tool_name": tool_name,
            "tool_result": tool_result,
        });

        let instructions = Self::assemble_instructions();

        let schema_value: serde_json::Value =
            serde_json::from_str(Self::output_schema()).map_err(|e| {
                format!("Failed to parse output schema JSON: {}", e)
            })?;

        let response = client
            .call_llm_structured(&instructions, &input_data, Some(&schema_value))
            .await?;

        let text = match response.text {
            Some(t) if !t.is_empty() => t,
            _ => return Err("Empty response from LLM".to_string()),
        };

        let refined: RefinedToolResult = serde_json::from_str(&text).map_err(|e| {
            format!("Failed to parse refined result JSON: {}", e)
        })?;

        Ok(refined)
    }

    pub fn output_schema() -> &'static str {
        TOOL_RESULT_REFINER_SCHEMA
    }

    pub fn assemble_instructions() -> String {
        String::from(
            "你是工具执行结果提炼专家。你的职责是对代码探索工具返回的原始数据去噪提炼，输出可操作的探索线索，供下一轮探索决策使用。\n\
             \n\
             ## 用户问题\n\
             {question}\n\
             \n\
             ## 工具名称\n\
             {tool_name}\n\
             \n\
             ## 工具执行结果\n\
             {tool_result}\n\
             \n\
             ## 提炼规则\n\
             \n\
             1. **保留所有可操作实体**：文件路径、函数名、类名、方法名、关键变量名、模块名\n\
             2. **保留关键代码片段**：函数签名、类定义、核心逻辑（≤10 行为宜）\n\
             3. **保留统计信息**：匹配总数、文件数、行数\n\
             4. **剔除 JSON 冗余**：去掉 success、truncated 等元数据\n\
             5. **不总结、不判断、不评分**：只搬运和整理原始数据\n\
             6. **按工具类型调整要点**：\n\
                - search_content：Top 匹配的文件→行号→内容\n\
                - search_files：文件路径列表\n\
                - read_file：关键代码片段（函数/类/核心逻辑）\n\
                - list_dir：目录结构和关键文件\n\
                - file_info：统计信息\n\
                - execute_shell：输出中的文件路径/符号/统计\n\
             \n\
             ## 输出格式\n\
             只输出一个 JSON 对象：\n\
             {\"summary\": \"提炼后的可操作线索\"}",
        )
    }
}
