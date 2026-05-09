use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::context::exploration::ExplorationContextTool;
use crate::fast_explorer::explorer::FastExplorer;
use super::executor::ToolExecutor;
use super::search_files::SearchFilesTool;
use super::read_file::ReadFileTool;
use super::search_content::SearchContentTool;
use super::list_dir::ListDirTool;
use super::file_info::FileInfoTool;
use super::execute_shell::ExecuteShellTool;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolExecutor>>,
    project_root: PathBuf,
}

impl ToolRegistry {
    pub fn project_root(&self) -> &PathBuf { &self.project_root }

    pub fn new(project_root: PathBuf) -> Self {
        let mut registry = ToolRegistry {
            tools: HashMap::new(),
            project_root: project_root.clone(),
        };
        registry.register(Arc::new(SearchFilesTool::new(project_root.clone())));
        registry.register(Arc::new(ReadFileTool::new(project_root.clone())));
        registry.register(Arc::new(SearchContentTool::new(project_root.clone())));
        registry.register(Arc::new(ListDirTool::new(project_root.clone())));
        registry.register(Arc::new(FileInfoTool::new(project_root.clone())));
        registry.register(Arc::new(ExecuteShellTool::new(project_root.clone())));
        registry.register(Arc::new(FastExplorer::new(project_root.clone())));
        registry.register(Arc::new(ExplorationContextTool::new(
            "default".to_string(),
        )));
        registry
    }

    pub fn register(&mut self, tool: Arc<dyn ToolExecutor>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn execute(&self, tool_name: &str, params: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let tool = self.tools.get(tool_name).ok_or_else(|| {
            ToolError::new(ErrorCode::InternalError, format!("Unknown tool: {}", tool_name))
        })?;
        let input = ToolInput {
            tool_name: tool_name.to_string(),
            params,
            project_root: self.project_root.clone(),
        };
        tool.execute(input)
    }

    pub fn list_tools(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }
}
