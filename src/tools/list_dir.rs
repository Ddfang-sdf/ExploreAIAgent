use std::path::PathBuf;
use std::fs;
use serde::{Deserialize, Serialize};

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::common::path_manager::PathManager;
use crate::common::truncation::MAX_LIST_DIR_ITEMS;
use super::executor::ToolExecutor;

#[derive(Debug, Clone, Deserialize)]
pub struct ListDirParams {
    #[serde(default = "default_path")]
    pub path: String,
}

fn default_path() -> String {
    ".".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirItem {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirOutput {
    pub success: bool,
    pub items: Vec<DirItem>,
    pub truncated: bool,
}

pub struct ListDirTool {
    path_manager: PathManager,
}

impl ListDirTool {
    pub fn new(project_root: PathBuf) -> Self {
        ListDirTool {
            path_manager: PathManager::new(project_root),
        }
    }

    pub fn sort_items(items: &mut Vec<DirItem>) {
        items.sort_by(|a, b| {
            let a_is_hidden = a.name.starts_with('.');
            let b_is_hidden = b.name.starts_with('.');

            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    match (a_is_hidden, b_is_hidden) {
                        (false, true) => std::cmp::Ordering::Less,
                        (true, false) => std::cmp::Ordering::Greater,
                        _ => a.name.cmp(&b.name),
                    }
                }
            }
        });
    }
}

impl ToolExecutor for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List directory contents"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: ListDirParams = serde_json::from_value(input.params)
            .map_err(|e| ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e)))?;

        let resolved = self.path_manager.validate(&params.path)?;

        if !resolved.is_dir() {
            return Err(ToolError::new(
                ErrorCode::PathNotDirectory,
                format!("Path is not a directory: {}", params.path),
            ));
        }

        let read_dir = fs::read_dir(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ToolError::new(ErrorCode::PermissionDenied, format!("Permission denied: {}", params.path))
            } else {
                ToolError::new(ErrorCode::InternalError, e.to_string())
            }
        })?;

        let mut items = Vec::new();
        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let name = match entry.file_name().to_str() {
                Some(n) => n.to_string(),
                None => continue,
            };

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let is_dir = metadata.is_dir();
            let size = if is_dir { 0 } else { metadata.len() };

            items.push(DirItem { name, is_dir, size });
        }

        Self::sort_items(&mut items);

        let truncated = items.len() > MAX_LIST_DIR_ITEMS;
        if truncated {
            items.truncate(MAX_LIST_DIR_ITEMS);
        }

        let output = ListDirOutput {
            success: true,
            items,
            truncated,
        };

        Ok(ToolOutput::new(serde_json::to_value(output).unwrap())
            .with_truncated(truncated))
    }
}
