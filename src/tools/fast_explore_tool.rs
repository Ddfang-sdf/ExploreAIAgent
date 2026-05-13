use crate::tools::registry::ToolRegistry;

pub struct FastExploreTool;

impl FastExploreTool {
    pub async fn execute(
        keywords: &[String],
        registry: &ToolRegistry,
    ) -> Result<serde_json::Value, String> {
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
                fb.cmp(&fa)
            });
            matches["matches"] = serde_json::Value::Array(sorted);
        }

        Ok(matches)
    }
}
