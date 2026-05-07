
use crate::adapter::api_adapter::LlmStructuredClient;
use crate::context::exploration::{ExplorationRecord, ExplorationSummary};

pub const REFINER_SCHEMA: &str = r#"{
  "name": "exploration_refiner_response",
  "strict": true,
  "schema": {
    "type": "object",
    "properties": {
      "key_findings": { "type": "string" },
      "critical_files": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "path": { "type": "string" },
            "one_sentence_summary": { "type": "string" }
          },
          "required": ["path", "one_sentence_summary"],
          "additionalProperties": false
        }
      },
      "missing_info": { "type": "string" },
      "confidence": { "type": "number" }
    },
    "required": ["key_findings", "critical_files", "missing_info", "confidence"],
    "additionalProperties": false
  }
}"#;

pub struct ExplorationRefinerAgent;

impl ExplorationRefinerAgent {
    pub fn new() -> Self {
        ExplorationRefinerAgent
    }

    pub async fn refine(
        &self,
        user_question: &str,
        current_summary: &ExplorationSummary,
        recent_records: &[ExplorationRecord],
        target_token_limit: usize,
        client: &dyn LlmStructuredClient,
    ) -> Result<ExplorationSummary, String> {
        // Step 0: empty-data early return (defensive check)
        let summary_is_empty = current_summary.key_findings.is_empty()
            && current_summary.critical_files.is_empty();
        if summary_is_empty && recent_records.is_empty() {
            return Err(
                "no data to refine: both current_summary and recent_records are empty"
                    .to_string(),
            );
        }

        // Step 1: serialize input data (strip confidence from records)
        let records_value: Vec<serde_json::Value> = recent_records
            .iter()
            .map(|r| {
                let mut v = serde_json::to_value(r)
                    .map_err(|e| format!("Failed to serialize record: {}", e))?;
                if let Some(obj) = v.as_object_mut() {
                    obj.remove("confidence");
                }
                Ok(v)
            })
            .collect::<Result<_, String>>()?;

        let input_data = serde_json::json!({
            "user_question": user_question,
            "current_summary": current_summary,
            "recent_records": records_value,
            "target_token_limit": target_token_limit,
        });

        // Step 2: assemble core instruction text
        let instructions = Self::assemble_instructions();

        // Step 3: call adapter — all mode differences handled by adapter
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
            Some(_) => return Err("Empty response from LLM".to_string()),
            None => {
                if !response.tool_calls.is_empty() {
                    let tool_names: Vec<String> =
                        response.tool_calls.iter().map(|tc| tc.name.clone()).collect();
                    return Err(format!(
                        "Unexpected tool calls in refiner response: {}",
                        tool_names.join(", ")
                    ));
                }
                return Err("Empty response from LLM".to_string());
            }
        };

        let summary: ExplorationSummary =
            serde_json::from_str(&text).map_err(|e| {
                format!("Failed to parse refinement JSON: {}", e)
            })?;

        // Step 5: validate confidence
        if summary.confidence < 0.0 || summary.confidence > 1.0 {
            return Err(format!(
                "confidence out of range [0.0, 1.0]: {}",
                summary.confidence
            ));
        }

        Ok(summary)
    }

    pub fn output_schema() -> &'static str {
        REFINER_SCHEMA
    }

    pub fn assemble_instructions() -> String {
        String::from(
            "你是探索上下文精炼专家。对探索上下文进行增量精炼，输出极简、高质量的摘要。\n\
             \n\
             系统会以结构化数据的形式向你提供用户问题、当前已精炼摘要、最近探索记录和目标 Token 上限，请基于这些内容完成精炼。\n\
             \n\
             ## 增量精炼要求\n\
             \n\
             1. **增量融入**：必须以「当前已精炼摘要」为基础，只将「最近探索记录」中新增的重要信息融入。不要从零重新总结。如果当前摘要为空（首次精炼），则必须从探索记录中全新归纳总结，但所有信息筛选规则、关键文件处理规则、长度控制规则同等适用，不得因首次精炼而降低标准或跳步。\n\
             2. **信息筛选**：\n\
                - 优先保留：直接回答用户问题的代码片段位置、核心文件路径、关键发现。\n\
                - 坚决去除：重复信息、已证伪的线索、无关文件名、调试日志。\n\
             3. **关键文件处理规则**：\n\
                - 优先保留在探索记录中已被实际读取并返回有效内容的文件（记录中 `result_summary` 非空或有代码片段返回）。\n\
                - 丢弃仅在搜索中匹配到文件名、但从未被实际读取过的文件。\n\
                - 如果对某条信息的可靠性存疑，在 `missing_info` 中注明。\n\
             4. **长度控制**：输出摘要的总 Token 数必须控制在系统给定的目标上限以内。\n\
             \n\
             ## 输出格式（强制约束）\n\
             \n\
             你必须**只输出一个合法的 JSON 对象**，不要包裹任何标记、不要添加任何解释文字。JSON 对象必须包含以下四个字段，字段名不可更改：\n\
             \n\
             - `key_findings`：字符串，精炼后的核心发现总结。\n\
             - `critical_files`：数组，每个元素为 `{\"path\": \"文件路径\", \"one_sentence_summary\": \"一句话说明该文件的作用\"}`。如无相关文件则为空数组 `[]`。\n\
             - `missing_info`：字符串，仍缺失的关键信息。如无则为空字符串 `\"\"`。\n\
             - `confidence`：数字，综合置信度评分（0.0 到 1.0）。\n\
             \n\
             **示例输出**：\n\
             {\n\
               \"key_findings\": \"找到 BooleanValidator.java 和 BooleanParam 注解定义，探明 validate 方法通过 checkRequired 和 checkDefaultValue 实现校验\",\n\
               \"critical_files\": [\n\
                 {\"path\": \"core/validation/BooleanValidator.java\", \"one_sentence_summary\": \"包含 BooleanValidator 类，validate 方法实现了完整校验逻辑\"},\n\
                 {\"path\": \"annotation/BooleanParam.java\", \"one_sentence_summary\": \"定义 required 和 defaultValue 两个配置属性\"}\n\
               ],\n\
               \"missing_info\": \"defaultValue 的默认值装载机制尚未找到\",\n\
               \"confidence\": 0.85\n\
             }\n\
             \n\
             **警告**：如果你输出的不是合法 JSON，或者缺少上述四个字段中的任何一个，系统将拒绝你的输出并要求你重新生成。",
        )
    }
}
