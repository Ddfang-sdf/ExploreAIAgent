use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::adapter::api_adapter::{ApiAdapter, ToolDefinition};
use crate::common::config::ExplorationConfig;
use crate::tools::registry::ToolRegistry;

/// A single round of fast exploration history for SearchStrategyAgent.
/// Matches design doc section 4.1.2 {exploration_history} format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRoundRecord {
    pub round: usize,
    pub keywords: Vec<String>,
    pub key_findings: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchStrategyResult {
    pub key_findings: String,
    pub critical_files: Vec<CriticalFileRef>,
    pub missing_info: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalFileRef {
    pub path: String,
    pub summary: String,
}

pub struct SearchStrategyAgent {
    max_rounds: usize,
    _adapter: Arc<ApiAdapter>,
    _tool_registry: Arc<ToolRegistry>,
}

impl SearchStrategyAgent {
    pub fn new(adapter: Arc<ApiAdapter>, tool_registry: Arc<ToolRegistry>) -> Self {
        SearchStrategyAgent {
            max_rounds: 5,
            _adapter: adapter,
            _tool_registry: tool_registry,
        }
    }

    pub fn from_config(
        adapter: Arc<ApiAdapter>,
        tool_registry: Arc<ToolRegistry>,
        config: &ExplorationConfig,
    ) -> Self {
        SearchStrategyAgent {
            max_rounds: config.max_fast_explore_rounds,
            _adapter: adapter,
            _tool_registry: tool_registry,
        }
    }

    pub fn max_rounds(&self) -> usize {
        self.max_rounds
    }

    pub async fn execute_round(
        &self,
        question: &str,
        exploration_history: &[SearchRoundRecord],
        round: usize,
    ) -> Result<SearchStrategyResult, String> {
        // ======================================================================
        // Phase 1: Keywords design (call_llm_structured → forced JSON output)
        // ======================================================================
        let keywords_prompt = self.assemble_keywords_prompt(question, exploration_history, round);
        let keywords_response = self._adapter
            .call_llm_with_tools(
                &[serde_json::json!({"role": "system", "content": &keywords_prompt})],
                &[],
                Some(&serde_json::json!({"type": "json_object"})),
            )
            .await?;

        let keywords_text = keywords_response.text.as_deref().unwrap_or("");
        let keywords_json: serde_json::Value = serde_json::from_str(keywords_text)
            .map_err(|e| format!("Phase 1: Failed to parse keywords JSON: {}", e))?;

        let keywords: Vec<String> = keywords_json
            .get("keywords")
            .and_then(|k| k.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        // Empty keywords + first round → question unrelated to codebase
        if keywords.is_empty() {
            return Ok(SearchStrategyResult {
                key_findings: "问题与代码库无关".to_string(),
                critical_files: vec![],
                missing_info: "无".to_string(),
                confidence: 1.0,
            });
        }

        // ======================================================================
        // Phase 2: Execute fast_explorer + auto-record to ECT (code layer)
        // ======================================================================
        let truncated_keywords: Vec<String> = keywords.into_iter().take(5).collect();
        let fe_params = serde_json::json!({
            "keywords": truncated_keywords,
            "exclude_paths": [],
        });

        let fe_output = self._tool_registry
            .execute("fast_explorer", fe_params)
            .map_err(|e| format!("fast_explorer failed: {}", e))?;

        // Auto-record exploration result to ECT
        let _ = self._tool_registry.execute(
            "exploration_context_tool",
            serde_json::json!({
                "action": "write",
                "data": {
                    "type": "summary",
                    "source": "SearchStrategyAgent",
                    "data": {
                        "key_findings": "",
                        "critical_files": [],
                        "missing_info": "",
                        "confidence": 0.0,
                    }
                }
            }),
        );

        // ======================================================================
        // Phase 3: Evaluate with response_format (call_llm_structured)
        // ======================================================================
        let eval_prompt = self.assemble_evaluation_prompt(question, &fe_output.data);
        let eval_response = self._adapter
            .call_llm_with_tools(
                &[serde_json::json!({"role": "system", "content": &eval_prompt})],
                &[],
                Some(&serde_json::json!({"type": "json_object"})),
            )
            .await?;

        let eval_text = eval_response.text.as_deref().unwrap_or("");
        let result: SearchStrategyResult = serde_json::from_str(eval_text)
            .map_err(|e| format!("Phase 3: Failed to parse evaluation JSON: {}", e))?;

        // Validate confidence
        if result.confidence < 0.0 || result.confidence > 1.0 {
            return Err(format!(
                "confidence out of range [0.0, 1.0]: {}",
                result.confidence
            ));
        }

        Ok(result)
    }

    pub fn assemble_keywords_prompt(
        &self,
        question: &str,
        exploration_history: &[SearchRoundRecord],
        _round: usize,
    ) -> String {
        let template = String::from(
            "你是搜索策略专家。你的任务是根据用户问题设计关键词（2-5 个）。\n\
             \n\
             {question}\n\
             {exploration_history}\n\
             \n\
             ## 要求\n\
             \n\
             1. 从用户问题中直接提取核心概念，同时包含中英文关键词。\n\
             2. 参考「历史探索记录」中已尝试的关键词，避免重复。如果上一轮置信度低但方向正确，尝试调整关键词（如同义词、缩写、不同命名风格）。\n\
             3. 如果用户问题与代码库完全无关，返回空关键词列表。这包括：\n\
                - 通用知识提问（如\"美国总统是谁\"）\n\
                - 闲聊问候（如\"你好\"、\"今天天气怎么样\"）\n\
                - **关于AI助手本身的元问题**（如\"你是谁\"、\"你能做什么\"、\"你叫什么名字\"、\"介绍一下你自己\"、\"你的能力是什么\"）——这类问题问的是AI系统本身，不是工作目录下的代码库\n\
             \n\
             ## 输出格式（强制约束）\n\
             \n\
             你必须**只输出一个合法的 JSON 对象**，不要包裹任何标记、不要添加任何解释文字。JSON 对象包含以下字段：\n\
             \n\
             - `keywords`：字符串数组，2-5 个关键词。如果问题与代码库无关，返回空数组 `[]`。\n\
             \n\
             **示例输出**：\n\
             {\"keywords\": [\"项目结构\", \"project\", \"架构\", \"architecture\"]}",
        );

        let question_section = format!("## 用户问题\n{}", question);
        let prompt = template.replace("{question}", &question_section);

        if exploration_history.is_empty() {
            prompt.replace("{exploration_history}", "## 历史探索记录\n（首轮探索，无历史记录）")
        } else {
            let json = serde_json::to_string(exploration_history).unwrap_or_default();
            prompt.replace("{exploration_history}", &format!("## 历史探索记录\n{}", json))
        }
    }

    pub fn assemble_evaluation_prompt(
        &self,
        question: &str,
        exploration_result: &serde_json::Value,
    ) -> String {
        let template = String::from(
            "你是搜索策略专家。你的任务是评估以下探索数据与用户问题的相关性，给出置信度评分。\n\
             \n\
             {question}\n\
             {exploration_data}\n\
             \n\
             ## 评估标准\n\
             \n\
             检查探索数据中的匹配内容是否包含直接回答用户问题的信息。\n\
             \n\
             | 情况 | 建议置信度 |\n\
             | :--- | :--- |\n\
             | 问题与代码库无关（无需探索） | 1.0 |\n\
             | 找到直接答案（如相关代码片段、配置说明） | 0.8 - 1.0 |\n\
             | 找到相关信息，但需要进一步整合或确认 | 0.5 - 0.7 |\n\
             | 只找到文件名，没有实质内容 | 0.2 - 0.4 |\n\
             | 探索后确认项目不包含该功能 | 0.1 - 0.2 |\n\
             | 完全不相关或没有任何搜索结果 | 0.0 |\n\
             \n\
             ## 输出格式（强制约束）\n\
             \n\
             你必须**只输出一个合法的 JSON 对象**，不要包裹任何标记、不要添加任何解释文字。JSON 对象包含以下字段：\n\
             \n\
             - `key_findings`：本轮评估的总结。如果问题与代码库无关，应明确写出\"问题与代码库无关\"。如果进行了探索，则用用户的语言概括发现。\n\
             - `critical_files`：数组，每个元素为 `{\"path\": \"文件路径\", \"summary\": \"该文件如何帮助回答问题\"}`。如果未探索或未发现相关文件，则为空数组 `[]`。\n\
             - `missing_info`：字符串，说明当前还缺少哪些关键信息。如果问题无关或信息充足，可写\"无\"。\n\
             - `confidence`：数字，0.0 到 1.0 之间的置信度评分。\n\
             \n\
             **示例输出**：\n\
             {\"key_findings\": \"在 README.md 中确认项目是多 Agent 股票分析系统\", \"critical_files\": [{\"path\": \"README.md\", \"summary\": \"项目概述文档\"}], \"missing_info\": \"各 Agent 的具体实现逻辑尚未探索\", \"confidence\": 0.7}",
        );

        let question_section = format!("## 用户问题\n{}", question);
        let prompt = template.replace("{question}", &question_section);

        let data_str = serde_json::to_string(exploration_result).unwrap_or_default();
        prompt.replace("{exploration_data}", &format!("## 探索数据\n{}", data_str))
    }

    pub fn assemble_prompt(
        &self,
        question: &str,
        exploration_history: &[SearchRoundRecord],
        _round: usize,
    ) -> String {
        let template = String::from(
            "你是搜索策略专家。你的任务是判断用户问题是否需要进行代码探索，如果需要，则执行快速探索并输出评估结果；如果不需要，则直接输出一个表明\"无需探索\"的评估结果。\n\
             \n\
             {question}\n\
             {exploration_history}\n\
             {tools}\n\
             \n\
             ## 工作流程\n\
             \n\
             **第一步：判断问题相关性**\n\
             - 如果用户问题属于简单的问候、闲聊、或明显与当前工作目录下的代码库无关的通用知识提问，则你**无需调用任何搜索工具**。\n\
             - 你只需在最终的评估结果中，将置信度设为 `1.0`，并在 `key_findings` 中明确说明\"问题与代码库无关\"。\n\
             - 如果用户问题涉及代码功能、配置方式、模块用途等，需要进行探索才能回答，则继续执行以下步骤。\n\
             \n\
             **第二步：设计关键词**（2-5 个）\n\
             - 从用户问题中直接提取核心概念，同时包含中英文关键词。\n\
             - 参考「历史探索记录」中已尝试的关键词，避免重复。如果上一轮置信度低但方向正确，尝试调整关键词（如同义词、缩写、不同命名风格）。\n\
             \n\
             **第三步：调用 fast_explorer** 执行批量搜索。\n\
             - 该工具的具体使用方法已在工具说明中指明，请按规范调用。\n\
             \n\
             **第四步：评估搜索结果质量**\n\
             - `fast_explorer` 返回的搜索结果（匹配内容）会由系统自动捕获并存储，你无需在输出中重复这些原始数据。\n\
             - 检查这些匹配内容是否包含直接回答用户问题的信息。\n\
             - 如果搜索结果显示项目中没有与问题相关的代码（例如搜索\"爬虫\"但项目是一个纯校验框架），应在评估中明确指出\"探索发现项目不包含该功能\"，并给出低置信度。\n\
             - 按照以下标准给出置信度评分（0.0 到 1.0）：\n\
             \n\
             | 情况 | 建议置信度 |\n\
             | :--- | :--- |\n\
             | 问题与代码库无关（无需探索） | 1.0 |\n\
             | 找到直接答案（如相关代码片段、配置说明） | 0.8 - 1.0 |\n\
             | 找到相关信息，但需要进一步整合或确认 | 0.5 - 0.7 |\n\
             | 只找到文件名，没有实质内容 | 0.2 - 0.4 |\n\
             | 探索后确认项目不包含该功能 | 0.1 - 0.2 |\n\
             | 完全不相关或没有任何搜索结果 | 0.0 |\n\
             \n\
             **第五步：记录本轮发现（强制）**\n\
             - 如果你调用了 `fast_explorer`，则**必须调用** `exploration_context_tool` 将本轮的关键发现、关键文件、缺失信息和置信度保存下来。该工具的具体使用方法已在工具说明中指明，请严格按规范调用。\n\
             - **禁止在未调用此工具的情况下直接输出评估结果**。系统将拒绝未附带记录步骤的评估输出，并要求你重新执行。\n\
             - 如果你未调用任何搜索工具（问题与代码库无关），则**无需**调用此工具。\n\
             \n\
             **第六步：输出评估结果**\n\
             - 在完成上述所有步骤（尤其是强制记录步骤）后，你必须以结构化 JSON 格式输出最终的评估结果，包含以下字段：\n\
               - `key_findings`：本轮评估的总结。如果问题与代码库无关，应明确写出\"问题与代码库无关\"。如果进行了探索，则用用户的语言概括发现。\n\
               - `critical_files`：数组，每个元素为 `{\"path\": \"文件路径\", \"summary\": \"该文件如何帮助回答问题\"}`。如果未探索或未发现相关文件，则为空数组 `[]`。\n\
               - `missing_info`：字符串，说明当前还缺少哪些关键信息。如果问题无关，可写\"无\"。\n\
               - `confidence`：数字，0.0 到 1.0 之间的置信度评分。\n\
             \n\
             **输出前的检查清单（内部确认，不输出）**\n\
             在输出最终评估结果前，请确认：\n\
             1. 是否已调用 `fast_explorer`？\n\
                - 若是：是否已调用 `exploration_context_tool` 记录发现？\n\
                - 若否：是否确实因为问题与代码库无关而跳过了探索？\n\
             \n\
             只有在完成上述检查后，才可输出评估 JSON。",
        );

        // Replace {question}
        let question_section = format!("## 用户问题\n{}", question);
        let prompt = template.replace("{question}", &question_section);

        // Replace {exploration_history}
        let history_section = if exploration_history.is_empty() {
            "## 历史探索记录\n（首轮探索，无历史记录）".to_string()
        } else {
            let json = serde_json::to_string(exploration_history).unwrap_or_default();
            format!("## 历史探索记录\n{}", json)
        };
        let prompt = prompt.replace("{exploration_history}", &history_section);

        // Replace {tools}
        let tools = self.get_tools();
        let mut tools_text = String::from("## 可用工具\n\n");
        for tool in &tools {
            tools_text.push_str(&format!("### {}\n", tool.name));
            tools_text.push_str(&tool.description);
            tools_text.push('\n');
        }
        prompt.replace("{tools}", &tools_text)
    }

    pub fn get_tools(&self) -> Vec<ToolDefinition> {
        // v1.1: LLM no longer touches tools — tool calls are orchestrated by code.
        vec![]
    }
}
