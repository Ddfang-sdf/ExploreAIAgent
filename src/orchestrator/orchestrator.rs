use std::sync::Arc;

use crate::adapter::api_adapter::ApiAdapter;
use crate::agents::deep_explorer::{CollectedEvidence, DeepExplorer};
use crate::agents::exploration_refiner::ExplorationRefinerAgent;
use crate::agents::main_agent::MainAgent;
use crate::agents::quality_evaluator::{
    ExplorationAction, ExplorationQualityEvaluator, QualityEvaluatorInput,
};
use crate::agents::search_strategy::{SearchRoundRecord, SearchStrategyAgent};
use crate::common::config::ExplorationConfig;
use crate::context::exploration::{
    ExplorationContextTool, ExplorationRecord, ExplorationSummary,
};
use crate::conversation::manager::ConversationManager;
use crate::tools::registry::ToolRegistry;

pub const MAX_FAST_EXPLORE_ROUNDS: usize = 5;
pub const EARLY_TERMINATION_CONFIDENCE: f64 = 0.9;

pub struct Orchestrator {
    adapter: Arc<ApiAdapter>,
    tool_registry: Arc<ToolRegistry>,
    _conversation_manager: ConversationManager,
    max_fast_explore_rounds: usize,
    early_termination_confidence: f64,
}

impl Orchestrator {
    pub fn new(
        adapter: Arc<ApiAdapter>,
        tool_registry: Arc<ToolRegistry>,
        conversation_manager: ConversationManager,
    ) -> Self {
        Orchestrator {
            adapter,
            tool_registry,
            _conversation_manager: conversation_manager,
            max_fast_explore_rounds: MAX_FAST_EXPLORE_ROUNDS,
            early_termination_confidence: EARLY_TERMINATION_CONFIDENCE,
        }
    }

    pub fn from_config(
        adapter: Arc<ApiAdapter>,
        tool_registry: Arc<ToolRegistry>,
        conversation_manager: ConversationManager,
        config: &ExplorationConfig,
    ) -> Self {
        Orchestrator {
            adapter,
            tool_registry,
            _conversation_manager: conversation_manager,
            max_fast_explore_rounds: config.max_fast_explore_rounds,
            early_termination_confidence: config.early_termination_confidence,
        }
    }

    pub fn should_early_terminate(&self, confidence: f64, current_round: usize) -> bool {
        confidence >= self.early_termination_confidence
            && current_round < self.max_fast_explore_rounds
    }

    pub fn should_deep_explore(
        &self,
        action: &ExplorationAction,
        question_is_code_related: bool,
    ) -> bool {
        *action == ExplorationAction::DeepExplore && question_is_code_related
    }

    pub fn build_qe_input(
        exploration_context: &ExplorationContextTool,
    ) -> Result<QualityEvaluatorInput, String> {
        let current_summary = exploration_context
            .get_current_summary()
            .unwrap_or(ExplorationSummary {
                key_findings: String::new(),
                critical_files: vec![],
                missing_info: String::new(),
                confidence: 0.0,
            });

        let all_records = exploration_context.get_history();

        // Extract collected_evidence from DeepExplorer's ToolCall records
        let collected_evidence: Vec<CollectedEvidence> = all_records
            .iter()
            .filter_map(|r| match r {
                ExplorationRecord::ToolCall {
                    source,
                    result_summary,
                    params,
                    ..
                } if source == "DeepExplorer" => {
                    let file = params
                        .get("file")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let line = params
                        .get("lines")
                        .and_then(|v| v.as_str())
                        .or_else(|| params.get("line").and_then(|v| v.as_str()))
                        .unwrap_or("")
                        .to_string();
                    Some(CollectedEvidence {
                        file,
                        line,
                        code_snippet: String::new(),
                        relevance: result_summary.clone(),
                    })
                }
                _ => None,
            })
            .collect();

        Ok(QualityEvaluatorInput {
            current_summary,
            collected_evidence,
        })
    }

    pub fn build_exploration_data(
        exploration_context: &ExplorationContextTool,
    ) -> serde_json::Value {
        let summary = exploration_context.get_current_summary();
        let history: Vec<serde_json::Value> = exploration_context
            .get_history()
            .iter()
            .filter_map(|r| serde_json::to_value(r).ok())
            .collect();

        serde_json::json!({
            "current_summary": summary,
            "exploration_history": history,
        })
    }

    pub async fn run(
        &self,
        question: &str,
        exploration_context: &mut ExplorationContextTool,
    ) -> Result<String, String> {
        let ssa = SearchStrategyAgent::new(self.adapter.clone(), self.tool_registry.clone());
        let qe = ExplorationQualityEvaluator::new();
        let refiner = ExplorationRefinerAgent::new();
        let main_agent = MainAgent::new();
        let llm_client: &dyn crate::adapter::api_adapter::LlmStructuredClient =
            self.adapter.as_ref();

        // ===== Phase 1: Fast exploration (up to max_rounds) =====
        eprintln!("[1/3] 快速搜索代码库...");
        let mut history: Vec<SearchRoundRecord> = Vec::new();
        let mut question_is_code_related = false;

        for round in 1..=self.max_fast_explore_rounds {
            let result = ssa
                .execute_round(question, &history, round, exploration_context, self.adapter.as_ref())
                .await
                .map_err(|e| format!("SearchStrategyAgent round {}: {}", round, e))?;

            // Check if SSA found code-related content
            if !result.key_findings.contains("问题与代码库无关") {
                question_is_code_related = true;
            }

            // Record to ECT
            let record = ExplorationRecord::Summary {
                source: "SearchStrategyAgent".to_string(),
                data: ExplorationSummary {
                    key_findings: result.key_findings.clone(),
                    critical_files: vec![],
                    missing_info: result.missing_info.clone(),
                    confidence: result.confidence,
                },
                confidence: result.confidence,
                timestamp: chrono::Utc::now(),
            };
            let _ = exploration_context.write_record(record);

            // Append to round history
            history.push(SearchRoundRecord {
                round,
                keywords: vec![], // SSA doesn't expose keywords in result; refinement uses key_findings
                key_findings: result.key_findings,
                confidence: result.confidence,
            });

            // v1.2: Context refinement is now internal to SSA (done inside execute_round).
            // The orchestrator no longer manages SSA context compression.

            // Check early termination
            if self.should_early_terminate(result.confidence, round) {
                break;
            }
        }

        // ===== Phase 2: QE evaluation + conditional deep explore =====
        eprintln!("[2/3] 评估探索质量...");
        // Skip exploration when SSA determined the question is codebase-unrelated
        if !question_is_code_related {
            let _final_summary = ExplorationSummary {
                key_findings: "问题与代码库无关".to_string(),
                critical_files: vec![],
                missing_info: "无".to_string(),
                confidence: 1.0,
            };
            let answer = main_agent.generate_answer(question, "", &serde_json::json!({
                "current_summary": &_final_summary,
                "exploration_history": [],
            }), llm_client).await?;
            return Ok(answer);
        }

        let qe_input = Self::build_qe_input(exploration_context)?;
        let evaluation = qe
            .evaluate(question, &qe_input, llm_client)
            .await
            .map_err(|e| format!("QE evaluation: {}", e))?;

        // Write QE evaluation to ECT
        let qe_summary = ExplorationSummary {
            key_findings: evaluation.key_findings.clone(),
            critical_files: evaluation.critical_files.iter().map(|f| {
                crate::context::exploration::CriticalFile {
                    path: f.path.clone(),
                    one_sentence_summary: f.one_sentence_summary.clone(),
                }
            }).collect(),
            missing_info: evaluation.missing_info.clone(),
            confidence: evaluation.confidence,
        };
        let _ = exploration_context.write_record(ExplorationRecord::Summary {
            source: "ExplorationQualityEvaluator".to_string(),
            data: qe_summary.clone(),
            confidence: evaluation.confidence,
            timestamp: chrono::Utc::now(),
        });
        let _ = exploration_context.update_summary(qe_summary);

        // Conditional deep exploration
        let _final_summary = if self.should_deep_explore(&evaluation.action, question_is_code_related) {
            eprintln!("[*] 深度探索中...");
            let mut de = DeepExplorer::new();
            let current_key = evaluation.key_findings.clone();
            let de_result = de
                .execute(question, &ExplorationSummary {
                    key_findings: current_key,
                    critical_files: vec![],
                    missing_info: evaluation.missing_info.clone(),
                    confidence: evaluation.confidence,
                }, self.adapter.as_ref(), self.tool_registry.as_ref(), exploration_context)
                .await;

            let de_success = match de_result {
                Ok(ref result) => {
                    for evidence in &result.collected_evidence {
                        let _ = exploration_context.write_record(ExplorationRecord::ToolCall {
                            source: "DeepExplorer".to_string(),
                            tool: "read_file".to_string(),
                            params: serde_json::json!({"file": evidence.file, "line": evidence.line}),
                            result_summary: evidence.relevance.clone(),
                            confidence: 0.8,
                            timestamp: chrono::Utc::now(),
                        });
                    }
                    true
                }
                Err(ref e) => {
                    eprintln!("[WARN] 深度探索失败，回退到快速探索结果: {}", e);
                    false
                }
            };

            // Refine exploration context after DE (it grew during tool calls)
            if exploration_context.needs_compression() {
                let post_de_summary = exploration_context.get_current_summary()
                    .unwrap_or(ExplorationSummary { key_findings: String::new(), critical_files: vec![], missing_info: String::new(), confidence: 0.0 });
                let post_de_recent: Vec<ExplorationRecord> = exploration_context.get_history()
                    .into_iter().rev().take(15).collect();
                let target = ((crate::context::exploration::EXPLORATION_TOKEN_THRESHOLD as f64) * 0.10_f64).max(300.0) as usize;
                if let Ok(refined) = refiner.refine(question, &post_de_summary, &post_de_recent, target, llm_client).await {
                    let _ = exploration_context.update_summary(refined);
                }
            }

            if de_success {
                let qe_input2 = Self::build_qe_input(exploration_context)?;
                let eval2 = qe
                    .evaluate(question, &qe_input2, llm_client)
                    .await
                    .map_err(|e| format!("QE post-deep-explore: {}", e))?;

                ExplorationSummary {
                    key_findings: eval2.key_findings,
                    critical_files: eval2.critical_files.iter().map(|f| crate::context::exploration::CriticalFile {
                        path: f.path.clone(),
                        one_sentence_summary: f.one_sentence_summary.clone(),
                    }).collect(),
                    missing_info: eval2.missing_info,
                    confidence: eval2.confidence,
                }
            } else {
                // Fallback: re-use first QE evaluation when DE fails
                ExplorationSummary {
                    key_findings: evaluation.key_findings.clone(),
                    critical_files: evaluation.critical_files.iter().map(|f| crate::context::exploration::CriticalFile {
                        path: f.path.clone(),
                        one_sentence_summary: f.one_sentence_summary.clone(),
                    }).collect(),
                    missing_info: evaluation.missing_info.clone(),
                    confidence: evaluation.confidence,
                }
            }
        } else {
            ExplorationSummary {
                key_findings: evaluation.key_findings,
                critical_files: vec![],
                missing_info: evaluation.missing_info,
                confidence: evaluation.confidence,
            }
        };

        // ===== Phase 3: Generate answer =====
        eprintln!("[3/3] 生成答案...");
        let exploration_data = Self::build_exploration_data(exploration_context);
        let conversation_context = "".to_string(); // TODO: get from ConversationManager

        let answer = main_agent
            .generate_answer(question, &conversation_context, &exploration_data, llm_client)
            .await
            .map_err(|e| format!("MainAgent: {}", e))?;

        Ok(answer)
    }
}
