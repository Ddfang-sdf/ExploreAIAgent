use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ToolInput {
    pub tool_name: String,
    pub params: serde_json::Value,
    pub project_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_matches: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_lossy: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub success: bool,
    pub data: serde_json::Value,
    pub truncated: bool,
    pub metadata: Option<Metadata>,
}

impl ToolOutput {
    pub fn new(data: serde_json::Value) -> Self {
        ToolOutput {
            success: true,
            data,
            truncated: false,
            metadata: None,
        }
    }

    pub fn with_truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}
