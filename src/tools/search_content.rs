use std::path::PathBuf;
use std::fs;
use serde::{Deserialize, Serialize};
use regex::Regex;
use walkdir::WalkDir;
use glob::Pattern;

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::common::path_manager::PathManager;
use crate::common::truncation::{MAX_SEARCH_CONTENT_RESULTS, MAX_SEARCH_FILE_SIZE, MAX_CONTEXT_LINES};
use crate::tools::search_files::SearchFilesTool;
use crate::tools::read_file::ReadFileTool;
use super::executor::ToolExecutor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContentParams {
    pub pattern: String,
    #[serde(default)]
    pub file_pattern: Option<String>,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    #[serde(default = "default_true")]
    pub exclude_test_files: bool,
    #[serde(default)]
    pub context_lines: usize,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentMatch {
    pub file: String,
    pub line: u32,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_before: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_after: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContentOutput {
    pub success: bool,
    pub matches: Vec<ContentMatch>,
    pub truncated: bool,
}

pub struct SearchContentTool {
    path_manager: PathManager,
}

impl SearchContentTool {
    pub fn new(project_root: PathBuf) -> Self {
        SearchContentTool {
            path_manager: PathManager::new(project_root),
        }
    }
}

impl ToolExecutor for SearchContentTool {
    fn name(&self) -> &str {
        "search_content"
    }

    fn description(&self) -> &str {
        "Search text content in files with regex"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: SearchContentParams = serde_json::from_value(input.params)
            .map_err(|e| ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e)))?;

        let re = Regex::new(&params.pattern)
            .map_err(|e| ToolError::new(ErrorCode::InvalidPattern, format!("Invalid regex: {}", e)))?;

        let context_lines = std::cmp::min(params.context_lines, MAX_CONTEXT_LINES);

        let file_pattern = params.file_pattern.as_ref().map(|p| {
            Pattern::new(p)
        }).transpose().map_err(|e| {
            ToolError::new(ErrorCode::InvalidPattern, format!("Invalid file pattern: {}", e))
        })?;

        let exclude_patterns: Vec<Pattern> = params.exclude_paths.iter()
            .filter_map(|p| Pattern::new(p).ok())
            .collect();

        let canonical_root = self.path_manager.project_root().canonicalize()
            .map_err(|e| ToolError::new(ErrorCode::InternalError, e.to_string()))?;

        let mut matches = Vec::new();
        let mut truncated = false;

        let walker = WalkDir::new(&canonical_root).follow_links(true);
        let mut entries: Vec<walkdir::DirEntry> = Vec::new();

        for entry in walker.into_iter().filter_entry(|e| {
            if e.file_type().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    return !SearchFilesTool::is_skipped_directory(name);
                }
            }
            true
        }) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type().is_file() {
                entries.push(entry);
            }
        }

        entries.sort_by(|a, b| a.path().cmp(b.path()));

        for entry in &entries {
            if truncated {
                break;
            }

            let abs_path = match entry.path().canonicalize() {
                Ok(p) => p,
                Err(_) => continue,
            };

            let rel_path = match abs_path.strip_prefix(&canonical_root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };

            if let Some(ref fp) = file_pattern {
                if let Some(filename) = abs_path.file_name().and_then(|f| f.to_str()) {
                    if !fp.matches(filename) {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            if exclude_patterns.iter().any(|p| p.matches(&rel_path)) {
                continue;
            }

            if params.exclude_test_files && SearchFilesTool::is_test_file(&rel_path) {
                continue;
            }

            let metadata = match fs::metadata(&abs_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if metadata.len() > MAX_SEARCH_FILE_SIZE {
                continue;
            }

            let raw_bytes = match fs::read(&abs_path) {
                Ok(b) => b,
                Err(_) => continue,
            };

            if ReadFileTool::is_binary_file(&raw_bytes) {
                continue;
            }

            let text = match std::str::from_utf8(&raw_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => String::from_utf8_lossy(&raw_bytes).to_string(),
            };

            let all_lines: Vec<&str> = text.lines().collect();

            for (idx, line) in all_lines.iter().enumerate() {
                if re.is_match(line) {
                    let line_num = (idx + 1) as u32;
                    let content = line.trim().to_string();

                    let (ctx_before, ctx_after) = if context_lines > 0 {
                        let start = idx.saturating_sub(context_lines);
                        let before: Vec<String> = all_lines[start..idx]
                            .iter()
                            .map(|l| l.to_string())
                            .collect();

                        let end = std::cmp::min(idx + 1 + context_lines, all_lines.len());
                        let after: Vec<String> = all_lines[idx + 1..end]
                            .iter()
                            .map(|l| l.to_string())
                            .collect();

                        (Some(before), Some(after))
                    } else {
                        (None, None)
                    };

                    matches.push(ContentMatch {
                        file: rel_path.clone(),
                        line: line_num,
                        content,
                        context_before: ctx_before,
                        context_after: ctx_after,
                    });

                    if matches.len() >= MAX_SEARCH_CONTENT_RESULTS {
                        truncated = true;
                        break;
                    }
                }
            }
        }

        let output = SearchContentOutput {
            success: true,
            matches,
            truncated,
        };

        Ok(ToolOutput::new(serde_json::to_value(output).unwrap())
            .with_truncated(truncated))
    }
}
