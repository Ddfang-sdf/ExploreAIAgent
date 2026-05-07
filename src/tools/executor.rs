use crate::common::error::ToolError;
use crate::common::models::{ToolInput, ToolOutput};

pub trait ToolExecutor: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError>;
}
