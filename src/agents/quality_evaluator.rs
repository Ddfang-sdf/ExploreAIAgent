use serde::{Deserialize, Serialize};

use crate::adapter::api_adapter::LlmStructuredClient;
use crate::agents::deep_explorer::CollectedEvidence;
use crate::context::exploration::ExplorationSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityCriticalFile {
    pub path: String,
    pub one_sentence_summary: String,
}

/// Input data for the QualityEvaluator.
/// Matches design doc section 4.6 {exploration_data} format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityEvaluatorInput {
    pub current_summary: ExplorationSummary,
    #[serde(default)]
    pub collected_evidence: Vec<CollectedEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityEvaluation {
    pub key_findings: String,
    pub critical_files: Vec<QualityCriticalFile>,
    pub missing_info: String,
    pub confidence: f64,
    pub action: ExplorationAction,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplorationAction {
    Answer,
    DeepExplore,
}

pub const QUALITY_EVALUATOR_SCHEMA: &str = r#"{
  "name": "exploration_quality_evaluator_response",
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
      "confidence": { "type": "number" },
      "action": { "type": "string", "enum": ["answer", "deep_explore"] },
      "reason": { "type": "string" }
    },
    "required": ["key_findings", "critical_files", "missing_info", "confidence", "action", "reason"],
    "additionalProperties": false
  }
}"#;

pub struct ExplorationQualityEvaluator;

impl ExplorationQualityEvaluator {
    pub fn new() -> Self {
        ExplorationQualityEvaluator
    }

    pub async fn evaluate(
        &self,
        question: &str,
        exploration_data: &QualityEvaluatorInput,
        client: &dyn LlmStructuredClient,
    ) -> Result<QualityEvaluation, String> {
        // Step 1: serialize exploration_data + question → JSON Value.
        // The instruction template tells the LLM that the question and
        // exploration data arrive together as structured input.
        let input_data: serde_json::Value = serde_json::to_value(exploration_data)
            .map_err(|e| format!("Failed to serialize exploration data: {}", e))?;

        // Wrap question alongside the exploration data so the LLM receives both.
        let input_data = serde_json::json!({
            "question": question,
            "exploration_data": input_data,
        });

        // Step 2: assemble core instruction text
        let instructions = Self::assemble_instructions();

        // Step 3: call adapter — all mode differences handled by the adapter
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
                        "Unexpected tool calls in evaluation response: {}",
                        tool_names.join(", ")
                    ));
                }
                return Err("Empty response from LLM".to_string());
            }
        };

        let evaluation: QualityEvaluation = serde_json::from_str(&text).map_err(|e| {
            format!("Failed to parse evaluation JSON: {}", e)
        })?;

        // Step 5: validate
        if evaluation.confidence < 0.0 || evaluation.confidence > 1.0 {
            return Err(format!(
                "confidence out of range [0.0, 1.0]: {}",
                evaluation.confidence
            ));
        }

        Ok(evaluation)
    }

    pub fn output_schema() -> &'static str {
        QUALITY_EVALUATOR_SCHEMA
    }

    pub fn assemble_instructions() -> String {
        String::from(
            "你是探索质量评估专家。你的职责是分析已有探索数据与用户问题的相关性，判断这些数据是否足以回答用户问题，并生成可供回答使用的精准摘要。\n\
             \n\
             系统会以结构化数据的形式向你提供用户问题和待评估的探索数据，请基于这些内容完成评估。\n\
             \n\
             ## 工作流程\n\
             \n\
             1. **分析相关性**：逐一审查探索数据中的每条证据，判断其与用户问题的相关程度。\n\
             2. **提炼关键发现**：将分散的证据归纳为 1-3 条核心发现。必须基于实际探索到的数据，不要编造未发现的信息。\n\
             3. **识别核心文件**：列出对回答问题最有帮助的 1-3 个文件，并说明理由。\n\
             4. **指出缺失信息**：如果现有数据仍不足以完整回答问题，明确说明还缺少什么信息。\n\
             5. **给出置信度评分和行动建议**：基于现有数据的完整性和相关性，给出 0.0 到 1.0 的评分及 `action` 建议。\n\
             \n\
             **置信度评分参考**：\n\
             \n\
             | 情况 | 建议置信度 |\n\
             | :--- | :--- |\n\
             | 数据直接包含答案，可完整回答 | 0.8 - 1.0 |\n\
             | 数据高度相关，但需少量推理或补充 | 0.5 - 0.7 |\n\
             | 仅部分相关或仅有文件名，大量信息缺失 | 0.2 - 0.4 |\n\
             | 完全不相关或无有效数据 | 0.0 - 0.1 |\n\
             \n\
             ## 输出格式（强制约束）\n\
             \n\
             你必须只输出一个合法的 JSON 对象，不要包裹任何标记、不要添加任何解释文字。JSON 对象必须包含以下字段：\n\
             \n\
             - `key_findings`：字符串，探索数据的核心发现总结（使用用户的语言）。\n\
             - `critical_files`：数组，每个元素为 `{\"path\": \"文件路径\", \"one_sentence_summary\": \"一句话说明该文件如何帮助回答问题\"}`。如无相关文件则为空数组 `[]`。\n\
             - `missing_info`：字符串，仍缺失的关键信息。如数据已足够回答则为空字符串 `\"\"`。\n\
             - `confidence`：数字，0.0 到 1.0 之间的置信度评分。\n\
             - `action`：字符串，`\"answer\"` 或 `\"deep_explore\"`。当置信度达标或问题与代码库无关时建议 `\"answer\"`；当信息不足需要进一步深入探索时建议 `\"deep_explore\"`。\n\
             - `reason`：字符串，简要说明给出该评分和建议的理由。\n\
             \n\
             **示例输出**：\n\
             {\n\
               \"key_findings\": \"找到 BooleanValidator.java 和 BooleanParam 注解定义。探明 validate 方法通过 checkRequired 和 checkDefaultValue 实现校验，但 defaultValue 的默认值装载机制尚未找到。\",\n\
               \"critical_files\": [\n\
                 {\"path\": \"core/validation/BooleanValidator.java\", \"one_sentence_summary\": \"包含 BooleanValidator 类及完整校验逻辑\"},\n\
                 {\"path\": \"annotation/BooleanParam.java\", \"one_sentence_summary\": \"定义 required 和 defaultValue 两个配置属性\"}\n\
               ],\n\
               \"missing_info\": \"defaultValue 的默认值装载机制尚未找到，可能位于配置解析模块。\",\n\
               \"confidence\": 0.85,\n\
               \"action\": \"answer\",\n\
               \"reason\": \"核心校验逻辑已查明，整体信息已足够回答用户主要问题。\"\n\
             }\n\
             \n\
             **警告**：如果你输出的不是合法 JSON，或者缺少上述六个字段中的任何一个，系统将拒绝你的输出并要求你重新生成。",
        )
    }
}
