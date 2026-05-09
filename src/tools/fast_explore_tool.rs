use crate::adapter::api_adapter::LlmStructuredClient;
use crate::agents::exploration_refiner::ExplorationRefinerAgent;
use crate::agents::quality_evaluator::ExplorationQualityEvaluator;
use crate::context::exploration::{
    ExplorationContextTool, ExplorationRecord, ExplorationSummary,
};
use crate::tools::registry::ToolRegistry;

pub struct FastExploreTool;

impl FastExploreTool {
    /// v1.2: Pure code-layer tool.
    /// Flow: FastExplorer → ECT write → refine check → QE → confidence write → return
    pub async fn execute(
        keywords: &[String],
        registry: &ToolRegistry,
        ect: &ExplorationContextTool,
        qe_client: &dyn LlmStructuredClient,
    ) -> Result<serde_json::Value, String> {
        // Step 1: Run FastExplorer
        let fe_params = serde_json::json!({
            "keywords": keywords,
            "exclude_paths": [],
        });
        let fe_output = registry
            .execute("fast_explorer", fe_params)
            .map_err(|e| format!("fast_explorer failed: {}", e))?;
        let mut matches: serde_json::Value = fe_output.data;

        // Sort by file mtime (newest first, OpenCode-style)
        if let Some(matches_arr) = matches.get("matches").and_then(|v| v.as_array()) {
            let mut sorted: Vec<_> = matches_arr.iter().cloned().collect();
            let mut mtimes: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
            for m in &sorted {
                if let Some(file) = m.get("file").and_then(|v| v.as_str()) {
                    if !mtimes.contains_key(file) {
                        let full_path = registry.project_root().join(file);
                        let mtime = std::fs::metadata(&full_path)
                            .and_then(|meta| meta.modified())
                            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                            .unwrap_or(0);
                        mtimes.insert(file.to_string(), mtime);
                    }
                }
            }
            sorted.sort_by(|a, b| {
                let fa = a.get("file").and_then(|v| v.as_str()).and_then(|f| mtimes.get(f)).copied().unwrap_or(0);
                let fb = b.get("file").and_then(|v| v.as_str()).and_then(|f| mtimes.get(f)).copied().unwrap_or(0);
                fb.cmp(&fa) // newest first
            });
            matches["matches"] = serde_json::Value::Array(sorted);
        }

        // Step 2: Write FastExplorer result to ECT (direct write, not via registry)
        let _ = ect.write_record(ExplorationRecord::ToolCall {
            source: "fast_explore".to_string(),
            tool: "FastExplorer".to_string(),
            params: serde_json::json!({"keywords": keywords}),
            result_summary: serde_json::to_string(&matches).unwrap_or_default(),
            confidence: 0.5,
            timestamp: chrono::Utc::now(),
        });

        // Step 3: Context refinement check
        if ect.needs_compression() {
            let ect_summary = ect.get_current_summary().unwrap_or(ExplorationSummary {
                key_findings: String::new(),
                critical_files: vec![],
                missing_info: String::new(),
                confidence: 0.0,
            });
            let history = ect.get_history();
            let recent_records: Vec<_> = history.into_iter().rev().take(15).collect();
            let threshold = crate::context::exploration::EXPLORATION_TOKEN_THRESHOLD;
            let target = ((threshold as f64) * 0.10_f64).max(300.0) as usize;

            let refiner = ExplorationRefinerAgent::new();
            if let Ok(new_summary) = refiner
                .refine("", &ect_summary, &recent_records, target, qe_client)
                .await
            {
                let _ = ect.update_summary(new_summary);
            }
        }

        // Step 4: QE evaluation (truncate exploration_data to avoid LLM timeout)
        let qe_input = serde_json::json!({
            "total_matches": matches.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
            "top_matches": matches.get("matches").and_then(|v| v.as_array()).map(|a| {
                a.iter().take(10).cloned().collect::<Vec<_>>()
            }).unwrap_or_default(),
        });
        let qe = ExplorationQualityEvaluator::new();

        let confidence = match qe.evaluate("", &qe_input, qe_client).await {
            Ok(summary) => {
                // Write QE confidence to ECT (direct write)
                let _ = ect.write_record(ExplorationRecord::Summary {
                    source: "ExplorationQualityEvaluator".to_string(),
                    data: ExplorationSummary {
                        key_findings: summary.key_findings,
                        critical_files: summary.critical_files.iter().map(|f| {
                            crate::context::exploration::CriticalFile {
                                path: f.path.clone(),
                                one_sentence_summary: f.one_sentence_summary.clone(),
                            }
                        }).collect(),
                        missing_info: summary.missing_info,
                        confidence: summary.confidence,
                    },
                    confidence: summary.confidence,
                    timestamp: chrono::Utc::now(),
                });
                summary.confidence
            }
            Err(_) => 0.5, // QE failure → default confidence
        };

        // Step 5: Trim matches for return (keep top results only)
        let top_matches = qe_input["top_matches"].clone();
        let result = serde_json::json!({
            "matches": top_matches,
            "key_findings": "",
            "critical_files": [],
            "confidence": confidence,
        });
        Ok(result)
    }
}
