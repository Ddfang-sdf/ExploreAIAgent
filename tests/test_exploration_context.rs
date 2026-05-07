use explore_ai_agent::context::exploration::*;
use chrono::Utc;

#[test]
fn exploration_context_new_session() {
    let ctx = ExplorationContextTool::new("session-1".to_string());
    assert_eq!(ctx.session_id(), "session-1");
    assert!(ctx.get_current_summary().is_none());
    assert!(ctx.get_history().is_empty());
    assert_eq!(ctx.total_token_count(), 0);
}

#[test]
fn exploration_context_write_tool_call_record() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    let record = ExplorationRecord::ToolCall {
        source: "DeepExplorer".to_string(),
        tool: "read_file".to_string(),
        params: serde_json::json!({"file": "src/main.rs"}),
        result_summary: "Found fn main at line 3".to_string(),
        confidence: 0.85,
        timestamp: Utc::now(),
    };

    let result = ctx.write_record(record);
    assert!(result.is_ok());
    assert_eq!(ctx.get_history().len(), 1);
}

#[test]
fn exploration_context_write_summary_record() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    let record = ExplorationRecord::Summary {
        source: "SearchStrategyAgent".to_string(),
        data: ExplorationSummary {
            key_findings: "Found BooleanValidator".to_string(),
            critical_files: vec![CriticalFile {
                path: "src/validator.rs".to_string(),
                one_sentence_summary: "Contains validator logic".to_string(),
            }],
            missing_info: "Missing config details".to_string(),
            confidence: 0.6,
        },
        confidence: 0.6,
        timestamp: Utc::now(),
    };

    let result = ctx.write_record(record);
    assert!(result.is_ok());
    assert_eq!(ctx.get_history().len(), 1);
}

#[test]
fn exploration_context_record_truncation_at_8000_chars() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Create a record with very long result_summary (> 8000 chars)
    let long_summary = "x".repeat(9000);
    let record = ExplorationRecord::ToolCall {
        source: "DeepExplorer".to_string(),
        tool: "search_content".to_string(),
        params: serde_json::json!({"pattern": "test"}),
        result_summary: long_summary,
        confidence: 0.5,
        timestamp: Utc::now(),
    };

    let result = ctx.write_record(record);
    assert!(result.is_ok());

    let history = ctx.get_history();
    assert_eq!(history.len(), 1);
    // Serialized record should be <= 8000 chars
    let serialized = serde_json::to_string(&history[0]).unwrap();
    assert!(serialized.len() <= RECORD_MAX_CHARS + 100); // small tolerance for truncation marker
}

#[test]
fn exploration_context_update_summary_success() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    let new_summary = ExplorationSummary {
        key_findings: "New findings".to_string(),
        critical_files: vec![],
        missing_info: "".to_string(),
        confidence: 0.9,
    };

    let result = ctx.update_summary(new_summary.clone());
    assert!(result.is_ok());
    assert!(ctx.get_current_summary().is_some());
    assert_eq!(ctx.get_current_summary().unwrap().key_findings, "New findings");
}

#[test]
fn exploration_context_needs_compression() {
    let ctx = ExplorationContextTool::new("session-1".to_string());
    // Fresh context should not need compression
    assert!(!ctx.needs_compression());
}

#[test]
fn exploration_context_compress_by_confidence_protects_qe_summaries() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Add a QE summary (should be protected)
    let qe_record = ExplorationRecord::Summary {
        source: "ExplorationQualityEvaluator".to_string(),
        data: ExplorationSummary {
            key_findings: "QE evaluation".to_string(),
            critical_files: vec![],
            missing_info: "".to_string(),
            confidence: 0.8,
        },
        confidence: 0.8,
        timestamp: Utc::now(),
    };

    // Add several low-confidence tool call records
    for i in 0..10 {
        let record = ExplorationRecord::ToolCall {
            source: "DeepExplorer".to_string(),
            tool: "search_content".to_string(),
            params: serde_json::json!({"pattern": format!("pattern_{}", i)}),
            result_summary: format!("Result {}", i),
            confidence: 0.1,
            timestamp: Utc::now(),
        };
        let _ = ctx.write_record(record);
    }
    let _ = ctx.write_record(qe_record);

    ctx.compress_by_confidence();

    // QE summary should still be present
    let remaining = ctx.get_history();
    assert!(remaining.iter().any(|r| r.is_quality_evaluator_summary()),
        "ExplorationQualityEvaluator summaries should be protected from compression");
}

// ECT-019: confidence() accessor — ToolCall returns outer confidence,
// Summary returns inner data.confidence (per design doc section 2.1.3)
#[test]
fn exploration_record_confidence_accessor() {
    // ToolCall: returns outer confidence field
    let tool_call = ExplorationRecord::ToolCall {
        source: "DeepExplorer".to_string(),
        tool: "read_file".to_string(),
        params: serde_json::json!({}),
        result_summary: "test".to_string(),
        confidence: 0.75,
        timestamp: Utc::now(),
    };
    assert!((tool_call.confidence() - 0.75).abs() < f64::EPSILON);

    // Summary: returns inner data.confidence, not outer field
    let summary = ExplorationRecord::Summary {
        source: "SearchStrategyAgent".to_string(),
        data: ExplorationSummary {
            key_findings: String::new(),
            critical_files: vec![],
            missing_info: String::new(),
            confidence: 0.9,
        },
        confidence: 0.5, // outer field — should be ignored by confidence()
        timestamp: Utc::now(),
    };
    assert!(
        (summary.confidence() - 0.9).abs() < f64::EPSILON,
        "Summary confidence() must return inner data.confidence (0.9), not outer field (0.5)"
    );
}

#[test]
fn exploration_record_source_accessor() {
    let record = ExplorationRecord::Summary {
        source: "SearchStrategyAgent".to_string(),
        data: ExplorationSummary {
            key_findings: String::new(),
            critical_files: vec![],
            missing_info: String::new(),
            confidence: 0.5,
        },
        confidence: 0.5,
        timestamp: Utc::now(),
    };
    assert_eq!(record.source(), "SearchStrategyAgent");
}

// ECT-018: is_quality_evaluator_summary — dual condition verification
// Summary(QE) → true (dual match), ToolCall(QE) → false (ToolCall not protected)
#[test]
fn exploration_record_is_qe_summary() {
    // Summary variant + QE source → true (dual condition match)
    let qe_summary = ExplorationRecord::Summary {
        source: "ExplorationQualityEvaluator".to_string(),
        data: ExplorationSummary {
            key_findings: String::new(),
            critical_files: vec![],
            missing_info: String::new(),
            confidence: 0.8,
        },
        confidence: 0.8,
        timestamp: Utc::now(),
    };
    assert!(qe_summary.is_quality_evaluator_summary());

    // ToolCall variant + QE source → false (requires Summary variant, not just source match)
    let qe_toolcall = ExplorationRecord::ToolCall {
        source: "ExplorationQualityEvaluator".to_string(),
        tool: "read_file".to_string(),
        params: serde_json::json!({"file": "src/main.rs"}),
        result_summary: "test".to_string(),
        confidence: 0.8,
        timestamp: Utc::now(),
    };
    assert!(
        !qe_toolcall.is_quality_evaluator_summary(),
        "ToolCall with QE source should NOT be identified as QE summary (requires Summary variant)"
    );
}

#[test]
fn exploration_summary_serialization() {
    let summary = ExplorationSummary {
        key_findings: "Found key code".to_string(),
        critical_files: vec![CriticalFile {
            path: "src/main.rs".to_string(),
            one_sentence_summary: "Entry point".to_string(),
        }],
        missing_info: "Missing tests".to_string(),
        confidence: 0.85,
    };

    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(json["key_findings"], "Found key code");
    assert_eq!(json["confidence"], 0.85);
    assert!(json["critical_files"].is_array());

    // Round-trip
    let deserialized: ExplorationSummary = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.key_findings, summary.key_findings);
    assert_eq!(deserialized.critical_files.len(), 1);
}

#[test]
fn exploration_context_read_records_with_query() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    let record1 = ExplorationRecord::ToolCall {
        source: "DeepExplorer".to_string(),
        tool: "read_file".to_string(),
        params: serde_json::json!({"file": "src/boolean_validator.rs"}),
        result_summary: "Found BooleanValidator class with validation logic".to_string(),
        confidence: 0.8,
        timestamp: Utc::now(),
    };

    let record2 = ExplorationRecord::ToolCall {
        source: "DeepExplorer".to_string(),
        tool: "search_content".to_string(),
        params: serde_json::json!({"pattern": "StringValidator"}),
        result_summary: "Found StringValidator class".to_string(),
        confidence: 0.6,
        timestamp: Utc::now(),
    };

    let _ = ctx.write_record(record1);
    let _ = ctx.write_record(record2);

    // Query for "Boolean" should only return the BooleanValidator record
    let query = RecordQuery {
        keyword: Some("Boolean".to_string()),
        file: None,
        limit: Some(10),
    };

    let results = ctx.read_records(&query);
    assert!(!results.is_empty(), "Should match at least one record");
    // Verify filtering: all returned records must contain "Boolean" in serialized form
    for record in &results {
        let serialized = serde_json::to_string(record).unwrap();
        assert!(
            serialized.to_lowercase().contains("boolean"),
            "Filtered record should match keyword 'Boolean': {}",
            serialized
        );
    }
}

// ECT-005: truncation at valid UTF-8 boundary (multi-byte character at cutoff)
#[test]
fn exploration_context_record_truncation_at_utf8_boundary() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Build a result_summary that, when serialized as JSON, exceeds 8000 bytes
    // and the 8000-byte boundary falls within a multi-byte UTF-8 character.
    // Strategy: pad with 7995 ASCII chars, then append "中文结尾" (12 UTF-8 bytes).
    // The JSON serialization overhead pushes total > 8000, and the cutoff
    // should land within the multi-byte region.
    let padding = "x".repeat(7995);
    let long_summary = format!("{}{}", padding, "中文结尾测试内容");

    let record = ExplorationRecord::ToolCall {
        source: "DeepExplorer".to_string(),
        tool: "search_content".to_string(),
        params: serde_json::json!({"pattern": "test"}),
        result_summary: long_summary,
        confidence: 0.5,
        timestamp: Utc::now(),
    };

    let result = ctx.write_record(record);
    assert!(result.is_ok());

    let history = ctx.get_history();
    assert_eq!(history.len(), 1);

    // The serialized record must be valid JSON (no broken UTF-8 at cutoff)
    let serialized = serde_json::to_string(&history[0]).unwrap();
    // Verify it can be re-deserialized
    let _deserialized: ExplorationRecord = serde_json::from_str(&serialized)
        .expect("Truncated record should be valid JSON with intact UTF-8");
}

// ECT-008: compress_by_confidence on empty history does not panic
#[test]
fn exploration_context_compress_empty_history() {
    let ctx = ExplorationContextTool::new("session-1".to_string());
    let removed = ctx.compress_by_confidence();
    assert_eq!(removed, 0);
    assert!(ctx.get_history().is_empty());
}

// ECT-009: compress_by_confidence below threshold returns 0
#[test]
fn exploration_context_compress_below_threshold_noop() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Add a few short records (token count will be well below 5500)
    for i in 0..3 {
        let record = ExplorationRecord::ToolCall {
            source: "DeepExplorer".to_string(),
            tool: "read_file".to_string(),
            params: serde_json::json!({"file": format!("file_{}.rs", i)}),
            result_summary: format!("Found something at line {}", i),
            confidence: 0.5,
            timestamp: Utc::now(),
        };
        let _ = ctx.write_record(record);
    }

    let history_before = ctx.get_history().len();
    let removed = ctx.compress_by_confidence();
    assert_eq!(removed, 0);
    assert_eq!(ctx.get_history().len(), history_before);
}

// ECT-010: compress_by_confidence respects MIN_REMAINING_RECORDS
#[test]
fn exploration_context_compress_respects_min_remaining() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Add a QE summary (protected)
    let qe_record = ExplorationRecord::Summary {
        source: "ExplorationQualityEvaluator".to_string(),
        data: ExplorationSummary {
            key_findings: "QE evaluation".to_string(),
            critical_files: vec![],
            missing_info: "".to_string(),
            confidence: 0.9,
        },
        confidence: 0.9,
        timestamp: Utc::now(),
    };
    let _ = ctx.write_record(qe_record);

    // Add exactly 4 low-confidence records (below MIN_REMAINING_RECORDS=5).
    // Use long summaries so Token > 5500 even with only 4 records,
    // testing the boundary where starting count < MIN_REMAINING_RECORDS.
    let long_text = "Token padding to ensure the total exceeds the compression threshold. ".repeat(50);
    for i in 0..4 {
        let record = ExplorationRecord::ToolCall {
            source: "DeepExplorer".to_string(),
            tool: "search_content".to_string(),
            params: serde_json::json!({"pattern": format!("pattern_{}", i)}),
            result_summary: format!("Result {}: {}", i, long_text),
            confidence: 0.1,
            timestamp: Utc::now(),
        };
        let _ = ctx.write_record(record);
    }

    ctx.compress_by_confidence();

    // QE summary should be protected
    let remaining = ctx.get_history();
    assert!(
        remaining.iter().any(|r| r.is_quality_evaluator_summary()),
        "QE summaries must be protected from compression"
    );

    // With only 4 non-protected records (below MIN_REMAINING_RECORDS=5),
    // the compression loop checks: 4 < 5? Yes → stop immediately.
    // All 4 records should remain (none deleted).
    let non_protected = remaining
        .iter()
        .filter(|r| !r.is_quality_evaluator_summary())
        .count();
    assert_eq!(
        non_protected, 4,
        "All 4 non-protected records should remain when starting count is below MIN_REMAINING_RECORDS (5), got {}",
        non_protected
    );
}

// ECT-012: token count is recalculated after update_summary
#[test]
fn exploration_context_update_summary_recalculates_token_count() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Add exactly 3 records (per design doc ECT-012)
    for i in 0..3 {
        let record = ExplorationRecord::ToolCall {
            source: "DeepExplorer".to_string(),
            tool: "read_file".to_string(),
            params: serde_json::json!({"file": format!("file_{}.rs", i)}),
            result_summary: format!("Found relevant code at line {}", i),
            confidence: 0.7,
            timestamp: Utc::now(),
        };
        let _ = ctx.write_record(record);
    }

    let token_before = ctx.total_token_count();

    let new_summary = ExplorationSummary {
        key_findings: "Comprehensive findings about the codebase".to_string(),
        critical_files: vec![
            CriticalFile {
                path: "src/main.rs".to_string(),
                one_sentence_summary: "Entry point".to_string(),
            },
        ],
        missing_info: "Missing config details".to_string(),
        confidence: 0.85,
    };

    let _ = ctx.update_summary(new_summary);
    let token_after = ctx.total_token_count();

    // Token count should increase (summary added)
    assert!(token_after > token_before,
        "Token count should increase after update_summary: before={}, after={}", token_before, token_after);
}

// ECT-014: needs_compression returns true when token count exceeds threshold
#[test]
fn exploration_context_needs_compression_over_threshold() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Add many records with long summaries to exceed EXPLORATION_TOKEN_THRESHOLD (5500)
    let long_text = "This is a fairly long summary that contains substantial information. ".repeat(20);
    for i in 0..30 {
        let record = ExplorationRecord::ToolCall {
            source: "DeepExplorer".to_string(),
            tool: "read_file".to_string(),
            params: serde_json::json!({"file": format!("file_{}.rs", i)}),
            result_summary: format!("Round {}: {}", i, long_text),
            confidence: 0.5,
            timestamp: Utc::now(),
        };
        let _ = ctx.write_record(record);
    }

    assert!(ctx.needs_compression(),
        "Should need compression when token count exceeds 5500");
}

// ECT-007: small-scale QE protection — 1 QE summary + 5 low-confidence records
#[test]
fn exploration_context_compress_small_scale_qe_protection() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Add 5 low-confidence records
    for i in 0..5 {
        let record = ExplorationRecord::ToolCall {
            source: "DeepExplorer".to_string(),
            tool: "search_content".to_string(),
            params: serde_json::json!({"pattern": format!("pattern_{}", i)}),
            result_summary: format!("Result {}: found some code", i),
            confidence: 0.1,
            timestamp: Utc::now(),
        };
        let _ = ctx.write_record(record);
    }

    // Add 1 QE summary (protected)
    let qe_record = ExplorationRecord::Summary {
        source: "ExplorationQualityEvaluator".to_string(),
        data: ExplorationSummary {
            key_findings: "QE final evaluation".to_string(),
            critical_files: vec![],
            missing_info: "".to_string(),
            confidence: 0.85,
        },
        confidence: 0.85,
        timestamp: Utc::now(),
    };
    let _ = ctx.write_record(qe_record);

    ctx.compress_by_confidence();

    let remaining = ctx.get_history();
    // QE summary must always be preserved
    assert!(
        remaining.iter().any(|r| r.is_quality_evaluator_summary()),
        "QE summary must be protected from compression"
    );
}

// ECT-016: read_records respects limit parameter
#[test]
fn exploration_context_read_records_limit() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    // Write 10 records with the same keyword
    for i in 0..10 {
        let record = ExplorationRecord::ToolCall {
            source: "DeepExplorer".to_string(),
            tool: "search_content".to_string(),
            params: serde_json::json!({"pattern": format!("TARGET_MARKER_{}", i)}),
            result_summary: format!("TARGET_MARKER found at line {}", i),
            confidence: 0.5,
            timestamp: Utc::now(),
        };
        let _ = ctx.write_record(record);
    }

    let query = RecordQuery {
        keyword: Some("TARGET_MARKER".to_string()),
        file: None,
        limit: Some(3),
    };

    let results = ctx.read_records(&query);
    assert_eq!(results.len(), 3, "limit=3 should return exactly 3 records");
}

// ECT-017: read_records returns empty Vec when no records match
#[test]
fn exploration_context_read_records_no_match() {
    let ctx = ExplorationContextTool::new("session-1".to_string());

    let record = ExplorationRecord::ToolCall {
        source: "DeepExplorer".to_string(),
        tool: "read_file".to_string(),
        params: serde_json::json!({"file": "src/main.rs"}),
        result_summary: "Found main function".to_string(),
        confidence: 0.8,
        timestamp: Utc::now(),
    };
    let _ = ctx.write_record(record);

    let query = RecordQuery {
        keyword: Some("zzz_nonexistent_keyword_xyz".to_string()),
        file: None,
        limit: Some(10),
    };

    let results = ctx.read_records(&query);
    assert!(results.is_empty());
}

// ECT-018b: is_quality_evaluator_summary returns false for Summary with non-QE source
#[test]
fn exploration_record_summary_with_non_qe_source_is_not_qe_summary() {
    let record = ExplorationRecord::Summary {
        source: "SearchStrategyAgent".to_string(),
        data: ExplorationSummary {
            key_findings: String::new(),
            critical_files: vec![],
            missing_info: String::new(),
            confidence: 0.5,
        },
        confidence: 0.5,
        timestamp: Utc::now(),
    };
    assert!(
        !record.is_quality_evaluator_summary(),
        "Summary with non-QE source should NOT be identified as QE summary (source mismatch)"
    );
}
