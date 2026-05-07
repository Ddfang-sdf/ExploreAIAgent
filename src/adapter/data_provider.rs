use crate::context::exploration::ExplorationSummary;
use super::types::{ExplorationHistoryData, OutputSchema, ToolDefinition};

#[async_trait::async_trait]
pub trait DataProvider: Send + Sync {
    fn get_question(&self) -> String;
    fn get_exploration_history(&self) -> ExplorationHistoryData;
    fn get_current_summary(&self) -> ExplorationSummary;
    fn get_tools(&self) -> Vec<ToolDefinition>;
    fn get_output_schema(&self) -> Option<OutputSchema>;
    fn get_loop_warning(&self) -> Option<String>;
}
