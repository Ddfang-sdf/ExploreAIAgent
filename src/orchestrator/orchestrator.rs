use std::sync::Arc;

use crate::adapter::api_adapter::{ApiAdapter, LlmStructuredClient};
use crate::agents::deep_explorer::CollectedEvidence;
use crate::agents::quality_evaluator::{ExplorationAction, QualityEvaluatorInput};
use crate::common::config::{DeepExplorerConfig, ExplorationConfig, FastExploreConfig};
use crate::context::exploration::{
    ExplorationContextTool, ExplorationRecord, ExplorationSummary,
};
use crate::conversation::manager::ConversationManager;
use crate::agents::main_agent::{ShellExecutor, ToolDispatcher};
use crate::adapter::model::ModelAdapter;
use crate::adapter::types::ToolDefinition;
use crate::tools::registry::ToolRegistry;

pub struct ShellExec {
    pub registry: Arc<ToolRegistry>,
}

#[async_trait::async_trait]
impl ShellExecutor for ShellExec {
    async fn execute(&self, command: &str) -> Result<serde_json::Value, String> {
        let params = serde_json::json!({"command": command});
        self.registry
            .execute("execute_shell", params)
            .map(|r| r.data)
            .map_err(|e| e.error)
    }
}

#[derive(Clone)]
pub struct Orchestrator {
    adapter: Arc<ApiAdapter>,
    tool_registry: Arc<ToolRegistry>,
    _conversation_manager: ConversationManager,
    pub de_config: DeepExplorerConfig,
    pub fe_config: FastExploreConfig,
    pub shell_output_lines: usize,
    pub shell_output_bytes: usize,
    pub compact_token_threshold: Option<usize>,
    pub model_adapter: Option<Arc<dyn ModelAdapter>>,
}

impl Orchestrator {
    pub fn new(
        adapter: Arc<ApiAdapter>,
        tool_registry: Arc<ToolRegistry>,
        conversation_manager: ConversationManager,
        de_config: DeepExplorerConfig,
        fe_config: FastExploreConfig,
    ) -> Self {
        Orchestrator {
            adapter,
            tool_registry,
            _conversation_manager: conversation_manager,
            de_config,
            fe_config,
            shell_output_lines: 500,
            shell_output_bytes: 10240,
            compact_token_threshold: None,
            model_adapter: None,
        }
    }

    /// v1.2: from_config simplified — SSA round/confidence configs removed
    pub fn from_config(
        adapter: Arc<ApiAdapter>,
        tool_registry: Arc<ToolRegistry>,
        conversation_manager: ConversationManager,
        config: &ExplorationConfig,
        de_config: &DeepExplorerConfig,
        fe_config: &FastExploreConfig,
    ) -> Self {
        let mut orch = Orchestrator::new(adapter, tool_registry, conversation_manager, de_config.clone(), fe_config.clone());
        orch.compact_token_threshold = config.compact_token_threshold;
        orch
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
        conversation_context: &str,
        exploration_context: Arc<ExplorationContextTool>,
    ) -> Result<String, String> {
        use crate::agents::main_agent::{DeepExploreExecutor, FastExploreExecutor, MainAgent};
        use crate::tools::fast_explore_tool::FastExploreTool;
        use std::sync::Arc;
        let model_adapter = self.model_adapter.as_ref()
            .ok_or_else(|| "ModelAdapter not configured".to_string())?;

        // FastExploreExecutor impl — delegates to FastExploreTool
        struct FeExec {
            registry: Arc<ToolRegistry>,
            ect: Arc<ExplorationContextTool>,
            qe_client: Arc<dyn LlmStructuredClient>,
        }

        #[async_trait::async_trait]
        impl FastExploreExecutor for FeExec {
            async fn execute(&self, keywords: &[String]) -> Result<serde_json::Value, String> {
                FastExploreTool::execute(keywords, &self.registry, &self.ect, self.qe_client.as_ref()).await
            }
        }

        // DeepExploreExecutor impl — delegates to DeepExplorer
        struct DeExec {
            adapter: Arc<ApiAdapter>,
            model_adapter: Arc<dyn ModelAdapter>,
            registry: Arc<ToolRegistry>,
            de_config: DeepExplorerConfig,
        }

        #[async_trait::async_trait]
        impl DeepExploreExecutor for DeExec {
            async fn execute(
                &self,
                question: &str,
                _summary: Option<&serde_json::Value>,
            ) -> Result<serde_json::Value, String> {
                use crate::agents::deep_explorer::DeepExplorer;
                let mut de = DeepExplorer::from_config(&self.de_config);
                let result = de.execute(question,
                    self.adapter.as_ref(),
                    self.model_adapter.as_ref(),
                    &self.registry,
                ).await;
                result.map(|r| serde_json::to_value(r).unwrap_or_default())
            }
        }

        let fe_exec_holder;
        let fe_exec: Option<&dyn FastExploreExecutor> = if self.fe_config.enable {
            fe_exec_holder = FeExec {
                registry: self.tool_registry.clone(),
                ect: exploration_context.clone(),
                qe_client: self.adapter.clone(),
            };
            Some(&fe_exec_holder)
        } else {
            None
        };
        let de_exec_holder;
        let de_exec: Option<&dyn DeepExploreExecutor> = if self.de_config.enable {
            de_exec_holder = DeExec {
                adapter: self.adapter.clone(),
                model_adapter: model_adapter.clone(),
                registry: self.tool_registry.clone(),
                de_config: self.de_config.clone(),
            };
            Some(&de_exec_holder)
        } else {
            None
        };

        let shell_exec = ShellExec {
            registry: self.tool_registry.clone(),
        };

        let shell_only = fe_exec.is_none() && de_exec.is_none();

        // Build tool definitions for function calling
        let mut tools: Vec<ToolDefinition> = Vec::new();
        // execute_shell is always available
        tools.push(ToolDefinition {
            name: "execute_shell".into(),
            description: "执行只读 Shell 命令探索代码库。当前 Shell: bash (Windows)。可用命令: cat head tail less grep egrep fgrep find ls tree wc sort uniq cut tr awk sed file stat echo。禁止 >/dev/null 以外的重定向、禁止 rm/mv/cp/mkdir、禁止 ../ 路径穿越、禁止后台 &。".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "要执行的 shell 命令"}
                },
                "required": ["command"]
            }),
        });
        if fe_exec.is_some() {
            tools.push(ToolDefinition {
                name: "fast_explore".into(),
                description: "快速扫描代码库。根据关键词（2-5个，中英文兼顾）批量搜索，返回 matches、key_findings、critical_files、confidence。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "keywords": {"type": "array", "items": {"type": "string"}, "description": "搜索关键词列表（2-5个）"}
                    },
                    "required": ["keywords"]
                }),
            });
        }
        if de_exec.is_some() {
            tools.push(ToolDefinition {
                name: "deep_explore".into(),
                description: "深度代码探索。自主调用底层工具深入收集代码证据，最多75次内部工具调用。返回 critical_files、collected_evidence、missing_info。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "question": {"type": "string", "description": "要探索的子问题"},
                        "current_summary": {"type": "object", "description": "已有的探索摘要（可选）"}
                    },
                    "required": ["question"]
                }),
            });
        }

        // Base exploration tools — always available
        tools.push(ToolDefinition {
            name: "search_content".into(),
            description: "搜索文件内容（正则匹配）。返回匹配的文件路径和行号，按修改时间排序。优先用此工具定位代码，而非 read_file 整个文件。".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "正则搜索模式"},
                    "file_pattern": {"type": "string", "description": "可选文件名过滤（glob），如 *.py"},
                    "exclude_paths": {"type": "array", "items": {"type": "string"}, "description": "排除的目录"}
                },
                "required": ["pattern"]
            }),
        });
        tools.push(ToolDefinition {
            name: "search_files".into(),
            description: "按文件名模式查找文件（glob）。返回匹配的文件路径列表。".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "glob 模式，如 **/*.rs"}
                },
                "required": ["pattern"]
            }),
        });
        tools.push(ToolDefinition {
            name: "read_file".into(),
            description: "读取文件内容。返回文件内容或指定行范围。读大文件前先用 wc -l 检查行数，超过200行用 lines 参数分批读。".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "文件路径"},
                    "lines": {"type": "string", "description": "可选行范围，如 1-100"}
                },
                "required": ["file"]
            }),
        });
        tools.push(ToolDefinition {
            name: "list_dir".into(),
            description: "列出目录内容。".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "目录路径"}
                },
                "required": ["path"]
            }),
        });
        tools.push(ToolDefinition {
            name: "file_info".into(),
            description: "获取文件元信息（大小、修改时间等）。".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "文件路径"}
                },
                "required": ["file"]
            }),
        });

        // Build ToolDispatcher
        struct AgentDispatcher<'a> {
            shell: &'a ShellExec,
            registry: &'a ToolRegistry,
            fe: Option<&'a (dyn FastExploreExecutor + 'a)>,
            de: Option<&'a (dyn DeepExploreExecutor + 'a)>,
        }
        #[async_trait::async_trait]
        impl ToolDispatcher for AgentDispatcher<'_> {
            async fn dispatch(&self, tool_name: &str, arguments: &serde_json::Value) -> Result<serde_json::Value, String> {
                match tool_name {
                    "execute_shell" => {
                        let cmd = arguments.get("command").and_then(|c| c.as_str()).unwrap_or("");
                        self.shell.execute(cmd).await
                    }
                    "fast_explore" => {
                        if let Some(fe) = self.fe {
                            let keywords: Vec<String> = arguments.get("keywords")
                                .and_then(|k| k.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default();
                            fe.execute(&keywords).await
                        } else {
                            Err("fast_explore 未启用".into())
                        }
                    }
                    "deep_explore" => {
                        if let Some(de) = self.de {
                            let question = arguments.get("question").and_then(|q| q.as_str()).unwrap_or("");
                            let summary = arguments.get("current_summary");
                            de.execute(question, summary).await
                        } else {
                            Err("deep_explore 未启用".into())
                        }
                    }
                    // Base tools: route to ToolRegistry
                    name @ ("search_content" | "search_files" | "read_file" | "list_dir" | "file_info") => {
                        self.registry.execute(name, arguments.clone())
                            .map(|o| o.data)
                            .map_err(|e| e.error)
                    }
                    _ => Err(format!("未知工具: {}", tool_name)),
                }
            }
        }
        let dispatcher = AgentDispatcher {
            shell: &shell_exec,
            registry: &self.tool_registry,
            fe: fe_exec,
            de: de_exec,
        };

        let main_agent = MainAgent::new();
        let answer = main_agent
            .run(
                question,
                conversation_context,
                tools,
                &dispatcher,
                self.adapter.clone(),
                model_adapter.as_ref(),
                exploration_context.clone(),
                shell_only,
                self.compact_token_threshold,
            )
            .await?;
        Ok(answer)
    }
}
