use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use glob::Pattern;

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::common::path_manager::PathManager;
use crate::common::truncation::MAX_SEARCH_FILES_RESULTS;
use super::executor::ToolExecutor;

#[derive(Debug, Clone, Deserialize)]
pub struct SearchFilesParams {
    pub pattern: String,
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default = "default_true")]
    pub exclude_test_files: bool,
}

fn default_path() -> String {
    ".".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilesOutput {
    pub success: bool,
    pub files: Vec<String>,
    pub truncated: bool,
}

pub struct SearchFilesTool {
    path_manager: PathManager,
}

impl SearchFilesTool {
    pub fn new(project_root: PathBuf) -> Self {
        SearchFilesTool {
            path_manager: PathManager::new(project_root),
        }
    }

    pub fn is_test_file(path: &str) -> bool {
        let normalized = path.replace('\\', "/");

        let test_dirs = ["test/", "tests/", "__tests__/", "__test__/", "spec/", "specs/"];
        for dir in &test_dirs {
            if normalized.contains(dir) {
                return true;
            }
        }

        if let Some(filename) = normalized.rsplit('/').next() {
            if let Some(stem) = filename.rsplit_once('.').map(|(s, _)| s) {
                if stem.ends_with("_test") || stem.ends_with("_spec") {
                    return true;
                }
                if stem.starts_with("test_") {
                    return true;
                }
                if stem.ends_with("Test") || stem.ends_with("Spec") {
                    return true;
                }
            }
        }

        false
    }

    pub fn is_skipped_directory(dir_name: &str) -> bool {
        const SKIPPED: &[&str] = &[
            ".git", ".svn", ".hg",
            "node_modules", "target", "build", "dist",
            ".idea", ".vscode",
            "__pycache__", ".tox", "vendor",
        ];
        SKIPPED.contains(&dir_name)
    }
}

impl ToolExecutor for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search files by glob pattern"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: SearchFilesParams = serde_json::from_value(input.params)
            .map_err(|e| ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e)))?;

        let search_root = self.path_manager.validate(&params.path)?;

        if search_root.is_file() {
            return Err(ToolError::new(
                ErrorCode::PathNotDirectory,
                format!("Path is not a directory: {}", params.path),
            ));
        }

        let pattern = Pattern::new(&params.pattern)
            .map_err(|e| ToolError::new(ErrorCode::InvalidPattern, format!("Invalid glob pattern: {}", e)))?;

        let canonical_root = self.path_manager.project_root().canonicalize()
            .map_err(|e| ToolError::new(ErrorCode::InternalError, e.to_string()))?;

        let mut files = Vec::new();
        let mut truncated = false;

        let walker = WalkDir::new(&search_root).follow_links(true);
        for entry in walker.into_iter().filter_entry(|e| {
            if e.file_type().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    return !Self::is_skipped_directory(name);
                }
            }
            true
        }) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = match entry.path().canonicalize() {
                Ok(p) => p,
                Err(_) => continue,
            };

            let rel_path = match abs_path.strip_prefix(&canonical_root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };

            if !pattern.matches(&rel_path) {
                if let Some(filename) = abs_path.file_name().and_then(|f| f.to_str()) {
                    if !pattern.matches(filename) {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            if params.exclude_test_files && Self::is_test_file(&rel_path) {
                continue;
            }

            files.push(rel_path);
            if files.len() >= MAX_SEARCH_FILES_RESULTS {
                truncated = true;
                break;
            }
        }

        files.sort();

        let output = SearchFilesOutput {
            success: true,
            files,
            truncated,
        };

        Ok(ToolOutput::new(serde_json::to_value(output).unwrap())
            .with_truncated(truncated))
    }
}
