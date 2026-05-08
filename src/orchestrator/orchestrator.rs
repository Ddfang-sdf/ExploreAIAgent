use std::sync::Arc;

use crate::adapter::api_adapter::{ApiAdapter, LlmStructuredClient};
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

pub struct Orchestrator {
    adapter: Arc<ApiAdapter>,
    tool_registry: Arc<ToolRegistry>,
    _conversation_manager: ConversationManager,
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
        }
    }

    /// v1.2: from_config simplified — SSA round/confidence configs removed
    pub fn from_config(
        adapter: Arc<ApiAdapter>,
        tool_registry: Arc<ToolRegistry>,
        conversation_manager: ConversationManager,
        _config: &ExplorationConfig,
    ) -> Self {
        Orchestrator::new(adapter, tool_registry, conversation_manager)
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

    /// v1.2: Thin scheduler — assemble modules, call MainAgent, return answer.
    pub async fn run(
        &self,
        question: &str,
        exploration_context: &ExplorationContextTool,
    ) -> Result<String, String> {
        use crate::agents::main_agent::{DeepExploreExecutor, FastExploreExecutor, MainAgent};
        use crate::tools::fast_explore_tool::FastExploreTool;
        use std::sync::Arc;

        let ect = Arc::new(exploration_context as *const ExplorationContextTool);
        // SAFETY: ECT outlives the async call (owned by CLI/main loop)
        let ect_ref: &'static ExplorationContextTool = unsafe { &*(exploration_context as *const ExplorationContextTool) };

        // FastExploreExecutor impl — delegates to FastExploreTool
        struct FeExec {
            registry: Arc<ToolRegistry>,
            ect: &'static ExplorationContextTool,
            qe_client: Arc<dyn LlmStructuredClient>,
        }

        #[async_trait::async_trait]
        impl FastExploreExecutor for FeExec {
            async fn execute(&self, keywords: &[String]) -> Result<serde_json::Value, String> {
                FastExploreTool::execute(keywords, &self.registry, self.ect, self.qe_client.as_ref()).await
            }
        }

        // DeepExploreExecutor impl — delegates to DeepExplorer
        struct DeExec {
            adapter: Arc<ApiAdapter>,
            registry: Arc<ToolRegistry>,
            ect: &'static ExplorationContextTool,
        }

        #[async_trait::async_trait]
        impl DeepExploreExecutor for DeExec {
            async fn execute(
                &self,
                question: &str,
                summary: Option<&serde_json::Value>,
            ) -> Result<serde_json::Value, String> {
                use crate::agents::deep_explorer::DeepExplorer;
                let current_summary = match summary.and_then(|s| serde_json::from_value(s.clone()).ok()) {
                    Some(s) => s,
                    None => ExplorationSummary {
                        key_findings: String::new(),
                        critical_files: vec![],
                        missing_info: String::new(),
                        confidence: 0.0,
                    },
                };
                let mut de = DeepExplorer::new();
                let result = de.execute(question, &current_summary,
                    self.adapter.as_ref(),
                    &self.registry,
                    self.ect,
                ).await;
                result.map(|r| serde_json::to_value(r).unwrap_or_default())
            }
        }

        let fe_exec = FeExec {
            registry: self.tool_registry.clone(),
            ect: ect_ref,
            qe_client: self.adapter.clone(),
        };
        let de_exec = DeExec {
            adapter: self.adapter.clone(),
            registry: self.tool_registry.clone(),
            ect: ect_ref,
        };

        let main_agent = MainAgent::new();
        let answer = main_agent
            .run(
                question,
                "", // conversation_context — TODO: wire CM
                &fe_exec,
                &de_exec,
                self.adapter.as_ref(),
            )
            .await?;
        Ok(answer)
    }
}
