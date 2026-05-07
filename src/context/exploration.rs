use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::tools::executor::ToolExecutor;

pub const EXPLORATION_TOKEN_THRESHOLD: usize = 5500;
pub const EXPLORATION_TOKEN_TARGET_RATIO: f64 = 0.70;
pub const MIN_REMAINING_RECORDS: usize = 5;
pub const RECORD_MAX_CHARS: usize = 8000;

// ---- data structures ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalFile {
    pub path: String,
    #[serde(alias = "summary")]
    pub one_sentence_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationSummary {
    pub key_findings: String,
    pub critical_files: Vec<CriticalFile>,
    pub missing_info: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ExplorationRecord {
    #[serde(rename = "summary")]
    Summary {
        source: String,
        data: ExplorationSummary,
        confidence: f64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "tool_call")]
    ToolCall {
        source: String,
        tool: String,
        params: serde_json::Value,
        result_summary: String,
        confidence: f64,
        timestamp: DateTime<Utc>,
    },
}

impl ExplorationRecord {
    pub fn confidence(&self) -> f64 {
        match self {
            ExplorationRecord::Summary { data, .. } => data.confidence,
            ExplorationRecord::ToolCall { confidence, .. } => *confidence,
        }
    }

    pub fn source(&self) -> &str {
        match self {
            ExplorationRecord::Summary { source, .. } => source,
            ExplorationRecord::ToolCall { source, .. } => source,
        }
    }

    pub fn is_quality_evaluator_summary(&self) -> bool {
        matches!(self, ExplorationRecord::Summary { source, .. } if source == "ExplorationQualityEvaluator")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationMetadata {
    pub total_token_count: usize,
    pub history_record_count: usize,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationContext {
    pub session_id: String,
    pub exploration_history: Vec<ExplorationRecord>,
    pub current_summary: Option<ExplorationSummary>,
    pub metadata: ExplorationMetadata,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExplorationContextToolInput {
    pub action: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub query: Option<RecordQuery>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecordQuery {
    pub keyword: Option<String>,
    pub file: Option<String>,
    pub limit: Option<usize>,
}

// ---- tool struct ----

pub struct ExplorationContextTool {
    context: Mutex<ExplorationContext>,
    backup_summary: Mutex<Option<ExplorationSummary>>,
    token_threshold: usize,
    token_target_ratio: f64,
    min_remaining_records: usize,
    record_max_chars: usize,
}

impl ExplorationContextTool {
    pub fn new(session_id: String) -> Self {
        ExplorationContextTool {
            context: Mutex::new(ExplorationContext {
                session_id,
                exploration_history: Vec::new(),
                current_summary: None,
                metadata: ExplorationMetadata {
                    total_token_count: 0,
                    history_record_count: 0,
                    last_updated: Utc::now(),
                },
            }),
            backup_summary: Mutex::new(None),
            token_threshold: EXPLORATION_TOKEN_THRESHOLD,
            token_target_ratio: EXPLORATION_TOKEN_TARGET_RATIO,
            min_remaining_records: MIN_REMAINING_RECORDS,
            record_max_chars: RECORD_MAX_CHARS,
        }
    }

    pub fn configure(
        &mut self,
        exploration: &crate::common::config::ExplorationConfig,
        context: &crate::common::config::ContextConfig,
    ) {
        self.token_threshold = exploration.token_threshold;
        self.token_target_ratio = exploration.token_target_ratio;
        self.min_remaining_records = context.min_remaining_records;
        self.record_max_chars = context.record_max_chars;
    }

    // ---- token estimation ----

    fn compute_total_tokens(context: &ExplorationContext) -> usize {
        let mut bytes: usize = 0;
        for record in &context.exploration_history {
            if let Ok(json) = serde_json::to_string(record) {
                bytes += json.len();
            }
        }
        if let Some(ref summary) = context.current_summary {
            if let Ok(json) = serde_json::to_string(summary) {
                bytes += json.len();
            }
        }
        bytes / 4
    }

    // ---- write_record ----

    pub fn write_record(&self, mut record: ExplorationRecord) -> Result<String, String> {
        // Truncate text fields in the source record until serialized JSON fits
        // within self.record_max_chars. This avoids producing invalid mid-string JSON.
        loop {
            let serialized = serde_json::to_string(&record)
                .map_err(|e| format!("Serialization error: {}", e))?;

            if serialized.len() <= self.record_max_chars {
                break;
            }

            let excess = serialized.len() - self.record_max_chars + 20; // safety margin

            match &mut record {
                ExplorationRecord::ToolCall {
                    ref mut result_summary,
                    ..
                } => {
                    let char_count = result_summary.chars().count();
                    if char_count == 0 {
                        break;
                    }
                    let trim_chars = std::cmp::min(excess, char_count);
                    let new_len = char_count - trim_chars;
                    // Find the byte position of the new char boundary
                    if new_len == 0 {
                        result_summary.clear();
                    } else {
                        let byte_pos = result_summary
                            .char_indices()
                            .nth(new_len)
                            .map(|(i, _)| i)
                            .unwrap_or(result_summary.len());
                        // Walk back to ensure we don't split a multi-byte char
                        let mut pos = byte_pos;
                        while pos > 0
                            && result_summary.as_bytes().get(pos).map_or(false, |b| b & 0xC0 == 0x80)
                        {
                            pos -= 1;
                        }
                        result_summary.truncate(pos);
                    }
                }
                ExplorationRecord::Summary {
                    ref mut data, ..
                } => {
                    // Trim key_findings first; the outer loop will re-check
                    // and trim missing_info on the next iteration if still needed
                    let char_count = data.key_findings.chars().count();
                    if char_count > 0 {
                        let trim_chars = std::cmp::min(excess, char_count);
                        let new_len = char_count.saturating_sub(trim_chars);
                        if new_len == 0 {
                            data.key_findings.clear();
                        } else {
                            let byte_pos = data
                                .key_findings
                                .char_indices()
                                .nth(new_len)
                                .map(|(i, _)| i)
                                .unwrap_or(data.key_findings.len());
                            let mut pos = byte_pos;
                            while pos > 0
                                && data.key_findings.as_bytes().get(pos).map_or(false, |b| b & 0xC0 == 0x80)
                            {
                                pos -= 1;
                            }
                            data.key_findings.truncate(pos);
                        }
                    } else {
                        // key_findings already empty — trim missing_info instead
                        let mi_chars = data.missing_info.chars().count();
                        if mi_chars > 0 {
                            let trim_chars = std::cmp::min(excess, mi_chars);
                            let new_len = mi_chars.saturating_sub(trim_chars);
                            if new_len == 0 {
                                data.missing_info.clear();
                            } else {
                                let byte_pos = data
                                    .missing_info
                                    .char_indices()
                                    .nth(new_len)
                                    .map(|(i, _)| i)
                                    .unwrap_or(data.missing_info.len());
                                let mut pos = byte_pos;
                                while pos > 0
                                    && data.missing_info.as_bytes().get(pos).map_or(false, |b| b & 0xC0 == 0x80)
                                {
                                    pos -= 1;
                                }
                                data.missing_info.truncate(pos);
                            }
                        } else {
                            break; // nothing left to trim
                        }
                    }
                }
            }
        }

        let mut ctx = self.context.lock().unwrap();
        ctx.exploration_history.push(record);
        ctx.metadata.history_record_count = ctx.exploration_history.len();
        ctx.metadata.total_token_count = Self::compute_total_tokens(&ctx);
        ctx.metadata.last_updated = Utc::now();

        Ok(format!(
            "{}-{}",
            ctx.exploration_history.last().map(|r| r.source()).unwrap_or("unknown"),
            ctx.metadata.last_updated.timestamp_millis()
        ))
    }

    // ---- read_records ----

    pub fn read_records(&self, query: &RecordQuery) -> Vec<ExplorationRecord> {
        let ctx = self.context.lock().unwrap();
        let mut results: Vec<ExplorationRecord> = ctx
            .exploration_history
            .iter()
            .filter(|record| {
                // Keyword filter: case-insensitive substring match on serialized JSON
                if let Some(ref keyword) = query.keyword {
                    let serialized = serde_json::to_string(record).unwrap_or_default();
                    if !serialized.to_lowercase().contains(&keyword.to_lowercase()) {
                        return false;
                    }
                }
                // File filter
                if let Some(ref file) = query.file {
                    let match_found = match record {
                        ExplorationRecord::ToolCall { params, .. } => {
                            params.get("file")
                                .and_then(|v| v.as_str())
                                .map(|f| f.contains(file.as_str()))
                                .unwrap_or(false)
                        }
                        ExplorationRecord::Summary { data, .. } => {
                            data.critical_files
                                .iter()
                                .any(|cf| cf.path.contains(file.as_str()))
                        }
                    };
                    if !match_found {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Sort by timestamp descending (newest first)
        results.sort_by(|a, b| {
            let ta = match a {
                ExplorationRecord::Summary { timestamp, .. } => timestamp,
                ExplorationRecord::ToolCall { timestamp, .. } => timestamp,
            };
            let tb = match b {
                ExplorationRecord::Summary { timestamp, .. } => timestamp,
                ExplorationRecord::ToolCall { timestamp, .. } => timestamp,
            };
            tb.cmp(ta)
        });

        // Apply limit
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        results
    }

    // ---- update_summary (copy-on-write) ----

    pub fn update_summary(&self, new_summary: ExplorationSummary) -> Result<(), String> {
        // Validate the new summary
        Self::validate_summary(&new_summary)?;

        let mut ctx = self.context.lock().unwrap();

        // Backup current summary
        let backup = ctx.current_summary.clone();
        *self.backup_summary.lock().unwrap() = backup;

        // Write new summary
        ctx.current_summary = Some(new_summary);

        // Validate the context after write
        if let Some(ref current) = ctx.current_summary {
            if let Err(e) = Self::validate_summary(current) {
                // Rollback
                let backup = self.backup_summary.lock().unwrap().take();
                ctx.current_summary = backup;
                return Err(format!("Validation failed after write (rolled back): {}", e));
            }
        }

        // Success — clear backup and update metadata
        *self.backup_summary.lock().unwrap() = None;
        ctx.metadata.total_token_count = Self::compute_total_tokens(&ctx);
        ctx.metadata.last_updated = Utc::now();

        Ok(())
    }

    fn validate_summary(summary: &ExplorationSummary) -> Result<(), String> {
        // Verify JSON round-trip
        let json = serde_json::to_string(summary)
            .map_err(|e| format!("Serialization failed: {}", e))?;
        let _parsed: ExplorationSummary = serde_json::from_str(&json)
            .map_err(|e| format!("Deserialization failed: {}", e))?;

        // confidence must be in [0.0, 1.0]
        if summary.confidence < 0.0 || summary.confidence > 1.0 {
            return Err(format!(
                "confidence out of range [0.0, 1.0]: {}",
                summary.confidence
            ));
        }
        Ok(())
    }

    // ---- compress_by_confidence ----

    pub fn compress_by_confidence(&self) -> usize {
        let mut ctx = self.context.lock().unwrap();

        if ctx.metadata.total_token_count <= self.token_threshold {
            return 0;
        }

        // Separate protected (QE summaries) from non-protected records
        let mut removed = 0;
        let _total = ctx.exploration_history.len();

        // Collect indices of non-protected records with their confidence
        let mut candidates: Vec<(usize, f64)> = ctx
            .exploration_history
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.is_quality_evaluator_summary())
            .map(|(i, r)| (i, r.confidence()))
            .collect();

        // Sort by confidence ascending (lowest confidence gets deleted first)
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let target_tokens = (self.token_threshold as f64 * self.token_target_ratio) as usize;
        let min_remaining = self.min_remaining_records;

        // Count non-protected records
        let non_protected_count = candidates.len();

        // Delete from lowest confidence, checking termination conditions
        let mut deleted_indices: Vec<usize> = Vec::new();
        for &(idx, _confidence) in &candidates {
            let remaining_non_protected = non_protected_count - deleted_indices.len();
            if remaining_non_protected < min_remaining {
                break;
            }

            // Check if we've reached the token target
            if ctx.metadata.total_token_count <= target_tokens {
                break;
            }

            deleted_indices.push(idx);
        }

        // Remove in reverse index order to maintain correctness
        deleted_indices.sort_by(|a, b| b.cmp(a));
        for idx in deleted_indices {
            ctx.exploration_history.remove(idx);
            removed += 1;
        }

        // Update metadata
        ctx.metadata.history_record_count = ctx.exploration_history.len();
        ctx.metadata.total_token_count = Self::compute_total_tokens(&ctx);
        ctx.metadata.last_updated = Utc::now();

        removed
    }

    // ---- needs_compression ----

    pub fn needs_compression(&self) -> bool {
        let ctx = self.context.lock().unwrap();
        ctx.metadata.total_token_count > self.token_threshold
    }

    // ---- accessors ----

    pub fn get_current_summary(&self) -> Option<ExplorationSummary> {
        self.context.lock().unwrap().current_summary.clone()
    }

    pub fn get_history(&self) -> Vec<ExplorationRecord> {
        self.context.lock().unwrap().exploration_history.clone()
    }

    pub fn total_token_count(&self) -> usize {
        self.context.lock().unwrap().metadata.total_token_count
    }

    pub fn get_context(&self) -> ExplorationContext {
        self.context.lock().unwrap().clone()
    }

    pub fn session_id(&self) -> String {
        self.context.lock().unwrap().session_id.clone()
    }
}

// ---- ToolExecutor implementation ----

impl ToolExecutor for ExplorationContextTool {
    fn name(&self) -> &str {
        "exploration_context_tool"
    }

    fn description(&self) -> &str {
        "Record exploration results or query historical exploration records"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let tool_input: ExplorationContextToolInput = serde_json::from_value(input.params)
            .map_err(|e| ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e)))?;

        match tool_input.action.as_str() {
            "write" => {
                let data = tool_input.data.ok_or_else(|| {
                    ToolError::new(ErrorCode::InternalError, "Missing data field for write action")
                })?;

                // Parse the data field to determine variant type
                let record_type = data
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let source = data
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let timestamp = Utc::now();

                let record = match record_type {
                    "tool_call" => {
                        let tool = data
                            .get("tool")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let params = data.get("params").cloned().unwrap_or(serde_json::Value::Null);
                        let result_summary = data
                            .get("result_summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let confidence = data
                            .get("confidence")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);

                        ExplorationRecord::ToolCall {
                            source: source.to_string(),
                            tool: tool.to_string(),
                            params,
                            result_summary,
                            confidence,
                            timestamp,
                        }
                    }
                    "summary" => {
                        let inner_data = data.get("data").cloned().unwrap_or(serde_json::Value::Null);
                        let exploration_summary: ExplorationSummary =
                            serde_json::from_value(inner_data).map_err(|e| {
                                ToolError::new(
                                    ErrorCode::InternalError,
                                    format!("Invalid summary data: {}", e),
                                )
                            })?;
                        let confidence = data
                            .get("confidence")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(exploration_summary.confidence);

                        ExplorationRecord::Summary {
                            source: source.to_string(),
                            data: exploration_summary,
                            confidence,
                            timestamp,
                        }
                    }
                    _ => {
                        return Err(ToolError::new(
                            ErrorCode::InternalError,
                            format!("Unknown record type: {}", record_type),
                        ));
                    }
                };

                let record_id = self.write_record(record).map_err(|e| {
                    ToolError::new(ErrorCode::InternalError, e)
                })?;

                let total_records = self.context.lock().unwrap().metadata.history_record_count;
                let output = serde_json::json!({
                    "success": true,
                    "record_id": record_id,
                    "total_records": total_records,
                });
                Ok(ToolOutput::new(output))
            }

            "read" => {
                let query = tool_input.query.unwrap_or(RecordQuery {
                    keyword: None,
                    file: None,
                    limit: None,
                });
                let records = self.read_records(&query);
                let total = records.len();
                let output = serde_json::json!({
                    "success": true,
                    "records": records,
                    "total": total,
                });
                Ok(ToolOutput::new(output))
            }

            _ => Err(ToolError::new(
                ErrorCode::InternalError,
                format!("Unknown action: {}", tool_input.action),
            )),
        }
    }
}
