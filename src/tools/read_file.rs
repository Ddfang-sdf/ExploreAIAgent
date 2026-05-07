use std::path::PathBuf;
use std::fs;
use serde::{Deserialize, Serialize};

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{Metadata, ToolInput, ToolOutput};
use crate::common::path_manager::PathManager;
use crate::common::truncation::{MAX_OUTPUT_BYTES, MAX_READ_FILE_LINES, MAX_LARGE_FILE_SIZE};
use super::executor::ToolExecutor;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum LineRanges {
    StringFormat(String),
    Structured { ranges: Vec<[u32; 2]> },
}

impl LineRanges {
    pub fn to_ranges(&self) -> Result<Vec<(u32, u32)>, String> {
        match self {
            LineRanges::StringFormat(s) => {
                let mut result = Vec::new();
                for part in s.split(',') {
                    let part = part.trim();
                    if let Some((start_str, end_str)) = part.split_once('-') {
                        let start: u32 = start_str.trim().parse()
                            .map_err(|_| format!("Invalid line number: {}", start_str))?;
                        let end: u32 = end_str.trim().parse()
                            .map_err(|_| format!("Invalid line number: {}", end_str))?;
                        result.push((start, end));
                    } else {
                        let line: u32 = part.parse()
                            .map_err(|_| format!("Invalid line number: {}", part))?;
                        result.push((line, line));
                    }
                }
                Ok(result)
            }
            LineRanges::Structured { ranges } => {
                Ok(ranges.iter().map(|r| (r[0], r[1])).collect())
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadFileParams {
    pub file: String,
    pub lines: Option<LineRanges>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileOutput {
    pub success: bool,
    pub content: String,
    pub lines: String,
    pub truncated: bool,
}

pub struct ReadFileTool {
    path_manager: PathManager,
}

impl ReadFileTool {
    pub fn new(project_root: PathBuf) -> Self {
        ReadFileTool {
            path_manager: PathManager::new(project_root),
        }
    }

    pub fn is_binary_file(data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }

        let check_region = |start: usize, len: usize| -> bool {
            let end = std::cmp::min(start + len, data.len());
            if start >= data.len() {
                return false;
            }
            data[start..end].contains(&0u8)
        };

        let sample_size = 8192;

        if data.len() < sample_size * 3 {
            return data.contains(&0u8);
        }

        if check_region(0, sample_size) {
            return true;
        }
        let mid = data.len() / 2;
        if check_region(mid, sample_size) {
            return true;
        }
        let tail_start = data.len().saturating_sub(sample_size);
        if check_region(tail_start, sample_size) {
            return true;
        }

        false
    }

    pub fn merge_ranges(ranges: &mut Vec<(u32, u32)>) -> Vec<(u32, u32)> {
        if ranges.is_empty() {
            return vec![];
        }
        ranges.sort_by_key(|r| r.0);
        let mut merged: Vec<(u32, u32)> = vec![ranges[0]];
        for &(start, end) in &ranges[1..] {
            let last = merged.last_mut().unwrap();
            if start <= last.1 + 1 {
                last.1 = std::cmp::max(last.1, end);
            } else {
                merged.push((start, end));
            }
        }
        merged
    }
}

impl ToolExecutor for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read file content with optional line ranges"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: ReadFileParams = serde_json::from_value(input.params)
            .map_err(|e| ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e)))?;

        let resolved = self.path_manager.validate(&params.file)?;

        if resolved.is_dir() {
            return Err(ToolError::new(
                ErrorCode::PathNotFile,
                format!("Path is a directory: {}", params.file),
            ));
        }

        let raw_bytes = fs::read(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ToolError::new(ErrorCode::PermissionDenied, format!("Permission denied: {}", params.file))
            } else {
                ToolError::new(ErrorCode::InternalError, e.to_string())
            }
        })?;

        if Self::is_binary_file(&raw_bytes) {
            let output = ReadFileOutput {
                success: true,
                content: "Binary file detected. Cannot display binary content.".to_string(),
                lines: "all".to_string(),
                truncated: false,
            };
            return Ok(ToolOutput::new(serde_json::to_value(output).unwrap()));
        }

        let has_line_ranges = params.lines.is_some();

        if !has_line_ranges && raw_bytes.len() as u64 > MAX_LARGE_FILE_SIZE {
            let output = ReadFileOutput {
                success: true,
                content: format!(
                    "File is too large ({:.1} MB). Please specify a line range to read a portion of the file.",
                    raw_bytes.len() as f64 / (1024.0 * 1024.0)
                ),
                lines: "all".to_string(),
                truncated: false,
            };
            return Ok(ToolOutput::new(serde_json::to_value(output).unwrap()));
        }

        let (text, encoding_lossy) = match std::str::from_utf8(&raw_bytes) {
            Ok(s) => (s.to_string(), false),
            Err(_) => (String::from_utf8_lossy(&raw_bytes).to_string(), true),
        };

        let all_lines: Vec<&str> = text.lines().collect();

        let (content, lines_desc, truncated) = if let Some(line_ranges) = params.lines {
            let mut ranges = line_ranges.to_ranges()
                .map_err(|e| ToolError::new(ErrorCode::InvalidLineRange, e))?;

            for &(start, end) in &ranges {
                if start == 0 {
                    return Err(ToolError::new(
                        ErrorCode::InvalidLineRange,
                        "Line numbers must start from 1".to_string(),
                    ));
                }
                if start > end {
                    return Err(ToolError::new(
                        ErrorCode::InvalidLineRange,
                        format!("Invalid range: start ({}) > end ({})", start, end),
                    ));
                }
            }

            let merged = Self::merge_ranges(&mut ranges);

            let mut selected_lines = Vec::new();
            for &(start, end) in &merged {
                let s = (start as usize).saturating_sub(1);
                let e = std::cmp::min(end as usize, all_lines.len());
                if s < all_lines.len() {
                    for line in &all_lines[s..e] {
                        selected_lines.push(*line);
                    }
                }
            }

            let desc = merged.iter()
                .map(|(s, e)| {
                    let actual_e = std::cmp::min(*e, all_lines.len() as u32);
                    if s == &actual_e { format!("{}", s) } else { format!("{}-{}", s, actual_e) }
                })
                .collect::<Vec<_>>()
                .join(",");

            let mut truncated = false;
            let mut result_lines = selected_lines;
            if result_lines.len() > MAX_READ_FILE_LINES {
                result_lines.truncate(MAX_READ_FILE_LINES);
                truncated = true;
            }

            let content = result_lines.join("\n");
            if content.len() > MAX_OUTPUT_BYTES {
                let (trunc_bytes, _) = crate::common::truncation::TruncationManager::truncate_output(
                    content.as_bytes(), MAX_OUTPUT_BYTES
                );
                (String::from_utf8_lossy(&trunc_bytes).to_string(), desc, true)
            } else {
                (content, desc, truncated)
            }
        } else {
            let mut truncated = false;
            let mut result_lines: Vec<&str> = all_lines.clone();
            if result_lines.len() > MAX_READ_FILE_LINES {
                result_lines.truncate(MAX_READ_FILE_LINES);
                truncated = true;
            }

            let content = result_lines.join("\n");
            if content.len() > MAX_OUTPUT_BYTES {
                let (trunc_bytes, _) = crate::common::truncation::TruncationManager::truncate_output(
                    content.as_bytes(), MAX_OUTPUT_BYTES
                );
                (String::from_utf8_lossy(&trunc_bytes).to_string(), "all".to_string(), true)
            } else {
                (content, "all".to_string(), truncated)
            }
        };

        let output = ReadFileOutput {
            success: true,
            content,
            lines: lines_desc,
            truncated,
        };

        let mut tool_output = ToolOutput::new(serde_json::to_value(output).unwrap())
            .with_truncated(truncated);

        if encoding_lossy {
            tool_output = tool_output.with_metadata(Metadata {
                execution_time_ms: None,
                total_matches: None,
                encoding_lossy: Some(true),
            });
        }

        Ok(tool_output)
    }
}
