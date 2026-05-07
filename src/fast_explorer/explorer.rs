use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::tools::executor::ToolExecutor;
use crate::tools::read_file::{ReadFileOutput, ReadFileTool};
use crate::tools::search_content::{
    ContentMatch, SearchContentOutput, SearchContentParams, SearchContentTool,
};

pub const MAX_FILES: usize = 20;
pub const MAX_MATCHES_PER_FILE: usize = 3;
pub const CONTEXT_LINES_AROUND: usize = 5;
/// Short-file threshold: files whose first match occurs at line ≤ this value
/// are de-prioritised in sorting (typically package-info.java or similar
/// stub files that contain only a package declaration).
pub const SHORT_FILE_LINE_THRESHOLD: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastExplorerMatch {
    pub file: String,
    pub line: String,
    pub content: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastExplorerOutput {
    pub matches: Vec<FastExplorerMatch>,
    pub total: usize,
    pub files_total: usize,
    pub files_sampled: usize,
    pub has_matches: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FastExplorerInput {
    pub keywords: Vec<String>,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
}

pub struct FastExplorer {
    project_root: std::path::PathBuf,
    search_content: SearchContentTool,
    read_file: ReadFileTool,
}

// ---- line-number helpers ----

fn parse_start_line(line: &str) -> u32 {
    line.split('-')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn parse_end_line(line: &str) -> u32 {
    line.split('-')
        .last()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

impl FastExplorer {
    pub fn new(project_root: std::path::PathBuf) -> Self {
        FastExplorer {
            search_content: SearchContentTool::new(project_root.clone()),
            read_file: ReadFileTool::new(project_root.clone()),
            project_root,
        }
    }

    /// Escape regex special characters in a keyword for literal matching.
    pub fn regex_escape_keyword(keyword: &str) -> String {
        regex::escape(keyword)
    }

    /// Build an OR regex pattern from escaped keywords joined with `|`.
    pub fn build_or_pattern(keywords: &[String]) -> String {
        keywords
            .iter()
            .map(|k| regex::escape(k))
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Deduplicate and merge adjacent matches (within ±1 line gap).
    /// Sorts by (file, start_line), then merges overlapping or adjacent ranges
    /// and removes exact duplicates. Operates in-place.
    pub fn dedup_matches(matches: &mut Vec<FastExplorerMatch>) {
        if matches.is_empty() {
            return;
        }

        let all = std::mem::take(matches);
        let mut sorted = all;
        sorted.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then_with(|| parse_start_line(&a.line).cmp(&parse_start_line(&b.line)))
        });

        let mut merged: Vec<FastExplorerMatch> = Vec::new();

        for m in sorted {
            if let Some(last) = merged.last_mut() {
                if last.file == m.file {
                    let last_start = parse_start_line(&last.line);
                    let last_end = parse_end_line(&last.line);
                    let m_start = parse_start_line(&m.line);
                    let m_end = parse_end_line(&m.line);

                    if last.line == m.line && last.content == m.content {
                        continue;
                    }

                    if m_start <= last_end + 1 {
                        let new_end = std::cmp::max(last_end, m_end);
                        last.line = if last_start == new_end {
                            format!("{}", last_start)
                        } else {
                            format!("{}-{}", last_start, new_end)
                        };
                        continue;
                    }
                }
            }
            merged.push(m);
        }

        *matches = merged;
    }

    // ---- file-level clustering & sorting (sections 4.4.1, 4.5) ----

    /// Cluster deduplicated matches by file. Within each file, keep at most
    /// `MAX_MATCHES_PER_FILE` matches (sorted by line ascending).
    /// Returns one group per file, preserving BTreeMap insertion order.
    fn cluster_by_file(matches: &[FastExplorerMatch]) -> Vec<Vec<FastExplorerMatch>> {
        let mut by_file: BTreeMap<String, Vec<FastExplorerMatch>> = BTreeMap::new();
        for m in matches {
            by_file.entry(m.file.clone()).or_default().push(m.clone());
        }
        for group in by_file.values_mut() {
            group.sort_by_key(|m| parse_start_line(&m.line));
            group.truncate(MAX_MATCHES_PER_FILE);
        }
        by_file.into_values().collect()
    }

    /// Sort file groups per section 4.5 three-level ordering:
    ///   L1: files whose first match ≤ SHORT_FILE_LINE_THRESHOLD go last
    ///   L2: first-match line ascending
    ///   L3: file path lexicographic (tie-breaker)
    fn sort_file_groups(groups: &mut [Vec<FastExplorerMatch>]) {
        groups.sort_by(|a, b| {
            let a_first = parse_start_line(&a[0].line);
            let b_first = parse_start_line(&b[0].line);
            let a_short = u32::from(a_first <= SHORT_FILE_LINE_THRESHOLD);
            let b_short = u32::from(b_first <= SHORT_FILE_LINE_THRESHOLD);

            // L1: short files go last (so b_short.cmp(&a_short) — reversed)
            b_short
                .cmp(&a_short)
                // L2: first-match line ascending
                .then(a_first.cmp(&b_first))
                // L3: file path lexicographic
                .then(a[0].file.cmp(&b[0].file))
        });
    }

    // ---- main pipeline ----

    /// Execute the full fast exploration pipeline:
    /// escape → build OR pattern → search_content → dedup →
    /// cluster by file → sort files → take top MAX_FILES →
    /// extract context → return.
    /// Errors from search_content are passed through transparently.
    pub fn execute_internal(
        &self,
        input: FastExplorerInput,
    ) -> Result<FastExplorerOutput, ToolError> {
        // 1. Build OR pattern from escaped keywords
        let pattern = Self::build_or_pattern(&input.keywords);

        // 2. Call search_content
        let search_params = SearchContentParams {
            pattern,
            file_pattern: None,
            exclude_paths: input.exclude_paths,
            exclude_test_files: true,
            context_lines: 0,
        };

        let tool_input = ToolInput {
            tool_name: "search_content".to_string(),
            params: serde_json::to_value(&search_params).map_err(|e| {
                ToolError::new(
                    ErrorCode::InternalError,
                    format!("Failed to serialize search params: {}", e),
                )
            })?,
            project_root: self.project_root.clone(),
        };

        let search_result = self.search_content.execute(tool_input)?;

        let search_output: SearchContentOutput =
            serde_json::from_value(search_result.data).map_err(|e| {
                ToolError::new(
                    ErrorCode::InternalError,
                    format!("Failed to parse search results: {}", e),
                )
            })?;

        // 3. Convert to FastExplorerMatch (no context yet)
        let mut matches: Vec<FastExplorerMatch> = search_output
            .matches
            .into_iter()
            .map(|m: ContentMatch| FastExplorerMatch {
                file: m.file,
                line: m.line.to_string(),
                content: m.content,
                context: String::new(),
            })
            .collect();

        // 4. Deduplicate and merge adjacent matches
        Self::dedup_matches(&mut matches);
        let total = matches.len();

        // 5. Cluster by file (≤ MAX_MATCHES_PER_FILE per file)
        let mut file_groups = Self::cluster_by_file(&matches);
        let files_total = file_groups.len();

        // 6. Sort file groups (section 4.5 three-level ordering)
        Self::sort_file_groups(&mut file_groups);

        // 7. Take top MAX_FILES files, flatten matches
        let files_sampled = file_groups.len().min(MAX_FILES);
        let top: Vec<FastExplorerMatch> = file_groups
            .into_iter()
            .take(MAX_FILES)
            .flatten()
            .collect();

        // 8. Extract context per section 4.6.3
        let final_matches = self.extract_contexts(top)?;

        Ok(FastExplorerOutput {
            matches: final_matches,
            total,
            files_total,
            files_sampled,
            has_matches: total > 0,
        })
    }

    // ---- private helpers ----

    fn extract_contexts(
        &self,
        matches: Vec<FastExplorerMatch>,
    ) -> Result<Vec<FastExplorerMatch>, ToolError> {
        let mut by_file: BTreeMap<String, Vec<FastExplorerMatch>> = BTreeMap::new();
        for m in matches {
            by_file.entry(m.file.clone()).or_default().push(m);
        }

        let mut result: Vec<FastExplorerMatch> = Vec::new();

        for (file, file_matches) in by_file {
            let mut ranges: Vec<(usize, u32, u32)> = Vec::new();
            for (idx, m) in file_matches.iter().enumerate() {
                let match_start = parse_start_line(&m.line);
                let match_end = parse_end_line(&m.line);
                let ctx_start = match_start
                    .saturating_sub(CONTEXT_LINES_AROUND as u32)
                    .max(1);
                let ctx_end = match_end.saturating_add(CONTEXT_LINES_AROUND as u32);
                ranges.push((idx, ctx_start, ctx_end));
            }

            if ranges.is_empty() {
                result.extend(file_matches);
                continue;
            }

            let batch_start = ranges.iter().map(|r| r.1).min().unwrap();
            let batch_end = ranges.iter().map(|r| r.2).max().unwrap();

            match self.read_lines(&file, batch_start, batch_end) {
                Ok(all_lines) => {
                    for (idx, ctx_start, ctx_end) in &ranges {
                        let mut m = file_matches[*idx].clone();
                        let offset_start =
                            (*ctx_start as usize).saturating_sub(batch_start as usize);
                        let offset_end =
                            (*ctx_end as usize).saturating_sub(batch_start as usize) + 1;
                        let slice_start = offset_start.min(all_lines.len());
                        let slice_end = offset_end.min(all_lines.len());
                        m.context = all_lines[slice_start..slice_end].join("\n");
                        result.push(m);
                    }
                }
                Err(_) => {
                    for m in file_matches {
                        let mut fallback = m;
                        fallback.context = String::new();
                        result.push(fallback);
                    }
                }
            }
        }

        Ok(result)
    }

    fn read_lines(
        &self,
        file: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<String>, ToolError> {
        let params = serde_json::json!({
            "file": file,
            "lines": format!("{}-{}", start, end),
        });

        let input = ToolInput {
            tool_name: "read_file".to_string(),
            params,
            project_root: self.project_root.clone(),
        };

        let result = self.read_file.execute(input)?;
        let output: ReadFileOutput =
            serde_json::from_value(result.data).map_err(|e| {
                ToolError::new(
                    ErrorCode::InternalError,
                    format!("Failed to parse read_file result: {}", e),
                )
            })?;

        if output.content.is_empty() {
            return Ok(Vec::new());
        }
        Ok(output.content.lines().map(|s| s.to_string()).collect())
    }
}

impl ToolExecutor for FastExplorer {
    fn name(&self) -> &str {
        "fast_explorer"
    }

    fn description(&self) -> &str {
        "Fast batch exploration: build OR pattern → search_content → dedup → cluster by file → sort → extract context → return"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: FastExplorerInput = serde_json::from_value(input.params).map_err(|e| {
            ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e))
        })?;

        if params.keywords.is_empty() {
            return Err(ToolError::new(
                ErrorCode::InternalError,
                "keywords must not be empty (1-5 words)".to_string(),
            ));
        }
        if params.keywords.len() > 5 {
            return Err(ToolError::new(
                ErrorCode::InternalError,
                "keywords must not exceed 5 words".to_string(),
            ));
        }

        let output = self.execute_internal(params)?;
        Ok(ToolOutput::new(serde_json::to_value(output).unwrap()))
    }
}
