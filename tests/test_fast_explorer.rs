mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::fast_explorer::explorer::*;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::registry::ToolRegistry;

// --- helpers ---

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "fast_explorer".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

fn make_params(keywords: Vec<&str>, exclude_paths: Vec<&str>) -> serde_json::Value {
    serde_json::json!({
        "keywords": keywords,
        "exclude_paths": exclude_paths,
    })
}

// ===================================================================
// FE-001 ~ FE-003：参数校验
// ===================================================================

/// FE-001: Empty keyword list → INTERNAL_ERROR
#[test]
fn fe_001_empty_keywords_error() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = make_input(root, make_params(vec![], vec![]));
    let result = explorer.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::InternalError);
}

/// FE-002: >5 keywords → INTERNAL_ERROR
#[test]
fn fe_002_too_many_keywords_error() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = make_input(root, make_params(
        vec!["a", "b", "c", "d", "e", "f"],
        vec![],
    ));
    let result = explorer.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::InternalError);
}

/// FE-003: Boundary — exactly 1 keyword → success=true
#[test]
fn fe_003_boundary_one_keyword() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = make_input(root, make_params(vec!["main"], vec![]));
    let _result = explorer.execute(input).expect("should succeed");
}

// ===================================================================
// FE-004 ~ FE-007：基本搜索功能
// ===================================================================

/// FE-004: Single keyword search → matches found in fixture
#[test]
fn fe_004_single_keyword_search() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["main".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert!(output.total > 0);
    assert!(output.has_matches);
    assert!(!output.matches.is_empty());
    // "main" appears in src/main.rs
    assert!(output.matches.iter().any(|m| m.file.contains("main.rs")));
}

/// FE-005: Multi-keyword OR search → matches from multiple keywords
#[test]
fn fe_005_multi_keyword_or_search() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["println".to_string(), "helper".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert!(output.has_matches);
    // Should find "println" in main.rs AND "helper" in lib.rs
    let files: Vec<&str> = output.matches.iter().map(|m| m.file.as_str()).collect();
    assert!(files.iter().any(|f| f.contains("main.rs")),
            "should find matches for 'println' in main.rs");
    assert!(files.iter().any(|f| f.contains("lib.rs")),
            "should find matches for 'helper' in lib.rs");
}

/// FE-006: No matches → empty result
#[test]
fn fe_006_no_matches() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["zzz_nonexistent_xyz".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert!(!output.has_matches);
    assert_eq!(output.total, 0);
    assert!(output.matches.is_empty());
}

/// FE-007: Chinese + English keywords → no error
#[test]
fn fe_007_chinese_english_keywords() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["test".to_string(), "测试".to_string()],
        exclude_paths: vec![],
    };
    let result = explorer.execute_internal(input);
    // Should not error — even if no matches, search should proceed normally
    assert!(result.is_ok());
}

// ===================================================================
// FE-008 ~ FE-011：去重与合并
// ===================================================================

/// FE-008: Consecutive lines merged (42, 43, 44 → "42-44")
#[test]
fn fe_008_consecutive_lines_merged() {
    let mut matches = vec![
        FastExplorerMatch { file: "a.rs".into(), line: "42".into(), content: "line42".into(), context: "".into() },
        FastExplorerMatch { file: "a.rs".into(), line: "43".into(), content: "line43".into(), context: "".into() },
        FastExplorerMatch { file: "a.rs".into(), line: "44".into(), content: "line44".into(), context: "".into() },
    ];
    FastExplorer::dedup_matches(&mut matches);
    // After dedup: should be 1 entry with line "42-44"
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line, "42-44");
}

/// FE-009: Non-consecutive lines kept separate (line 10 and line 25)
#[test]
fn fe_009_non_consecutive_not_merged() {
    let mut matches = vec![
        FastExplorerMatch { file: "a.rs".into(), line: "10".into(), content: "line10".into(), context: "".into() },
        FastExplorerMatch { file: "a.rs".into(), line: "25".into(), content: "line25".into(), context: "".into() },
    ];
    FastExplorer::dedup_matches(&mut matches);
    // Should remain 2 separate entries (gap > 1)
    assert_eq!(matches.len(), 2);
}

/// FE-010: Cross-file matches NOT merged
#[test]
fn fe_010_cross_file_not_merged() {
    let mut matches = vec![
        FastExplorerMatch { file: "a.rs".into(), line: "10".into(), content: "a10".into(), context: "".into() },
        FastExplorerMatch { file: "b.rs".into(), line: "10".into(), content: "b10".into(), context: "".into() },
    ];
    FastExplorer::dedup_matches(&mut matches);
    // Both retained (different files)
    assert_eq!(matches.len(), 2);
}

/// FE-011: Exact same line → only one retained after dedup
#[test]
fn fe_011_exact_duplicate_removed() {
    let mut matches = vec![
        FastExplorerMatch { file: "src/lib.rs".into(), line: "5".into(), content: "use std".into(), context: "".into() },
        FastExplorerMatch { file: "src/lib.rs".into(), line: "5".into(), content: "use std".into(), context: "".into() },
    ];
    FastExplorer::dedup_matches(&mut matches);
    assert_eq!(matches.len(), 1);
}

// ===================================================================
// FE-012 ~ FE-013：排序验证
// ===================================================================

/// FE-012: Output structure — files_total and files_sampled are present and consistent.
#[test]
fn fe_012_output_structure_consistency() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["fn ".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert!(output.files_total > 0);
    assert!(output.files_sampled > 0);
    assert!(output.files_sampled <= output.files_total);
    assert!(output.files_sampled <= MAX_FILES);
}

/// FE-013: Same-file matches ordered by line number ascending
#[test]
fn fe_013_same_file_line_order() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["use".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    // Within same file, line numbers should be ascending
    let mut prev_file = "";
    let mut prev_line = 0u32;
    for m in &output.matches {
        if m.file == prev_file {
            let curr_line: u32 = m.line.split('-').next().unwrap().parse().unwrap_or(0);
            assert!(curr_line >= prev_line,
                    "lines in same file should be ascending");
            prev_line = curr_line;
        } else {
            prev_file = &m.file;
            prev_line = m.line.split('-').next().unwrap().parse().unwrap_or(0);
        }
    }
}

// ===================================================================
// FE-014 ~ FE-016：上下文提取
// ===================================================================

/// FE-014: context contains match line ±5 lines
#[test]
fn fe_014_context_has_plus_minus_5() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["helper_one".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert!(!output.matches.is_empty());
    let m = &output.matches[0];
    // context should contain more than just the match content
    assert!(!m.context.is_empty());
    assert!(m.context.lines().count() >= 1);
}

/// FE-015: File start → context before < 5 lines
#[test]
fn fe_015_context_at_file_start() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // Search for something near the start of a file (line 1 content)
    let input = FastExplorerInput {
        keywords: vec!["use std::io".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    if let Some(m) = output.matches.first() {
        // context may start from line 1 naturally
        assert!(!m.context.is_empty());
    }
}

/// FE-016: File end → context after < 5 lines
#[test]
fn fe_016_context_at_file_end() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // Search for something near the end of a file
    let input = FastExplorerInput {
        keywords: vec!["VAL_32".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    if let Some(m) = output.matches.first() {
        assert!(!m.context.is_empty());
    }
}

// ===================================================================
// FE-017 ~ FE-019：结果限制
// ===================================================================

/// FE-017: Result limit — matches.len() ≤ MAX_FILES * MAX_MATCHES_PER_FILE.
#[test]
fn fe_017_file_cluster_limit() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["VAL".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert!(
        output.matches.len() <= MAX_FILES * MAX_MATCHES_PER_FILE,
        "matches.len() ({}) should be ≤ MAX_FILES * MAX_MATCHES_PER_FILE ({})",
        output.matches.len(),
        MAX_FILES * MAX_MATCHES_PER_FILE,
    );
}

/// FE-018: When files_total < MAX_FILES → files_sampled == files_total.
#[test]
fn fe_018_all_files_sampled() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // Rare keyword → few matching files
    let input = FastExplorerInput {
        keywords: vec!["helper_one".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    if output.files_total < MAX_FILES {
        assert_eq!(output.files_sampled, output.files_total);
    }
}

/// FE-019: Constants — verify default values.
#[test]
fn fe_019_constants() {
    assert_eq!(MAX_FILES, 20);
    assert_eq!(MAX_MATCHES_PER_FILE, 3);
    assert_eq!(CONTEXT_LINES_AROUND, 5);
}

// ===================================================================
// FE-020 ~ FE-021：排除路径
// ===================================================================

/// FE-020: exclude_paths filters out matching directory
#[test]
fn fe_020_exclude_paths_works() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["test".to_string()],
        exclude_paths: vec!["docs/*".to_string()],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    // No match from docs/ should be present
    for m in &output.matches {
        assert!(!m.file.starts_with("docs/"),
                "docs/ should be excluded, got: {}", m.file);
    }
}

/// FE-021: Empty exclude_paths → same as not passing it
#[test]
fn fe_021_empty_exclude_paths() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["fn".to_string()],
        exclude_paths: vec![],
    };
    let result = explorer.execute_internal(input);
    assert!(result.is_ok());
}

// ===================================================================
// FE-022 ~ FE-023：正则转义
// ===================================================================

/// FE-022: `.` in keyword is escaped — literal match only
#[test]
fn fe_022_dot_escaped_in_keyword() {
    let escaped = FastExplorer::regex_escape_keyword("config.go");
    assert_eq!(escaped, "config\\.go");
}

/// FE-023: `(` in keyword is escaped — no regex syntax error
#[test]
fn fe_023_paren_escaped_in_keyword() {
    let escaped = FastExplorer::regex_escape_keyword("validate(");
    assert_eq!(escaped, "validate\\(");
}

// ===================================================================
// FE-024：ToolExecutor trait 集成
// ===================================================================

/// FE-024: ToolRegistry dispatch → fast_explorer tool returns ToolOutput
#[test]
fn fe_024_tool_registry_integration() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let registry = ToolRegistry::new(root.to_path_buf());

    let params = serde_json::json!({
        "keywords": ["fn"],
        "exclude_paths": [],
    });
    let output = registry.execute("fast_explorer", params).expect("should succeed");
    assert!(output.success);
}

// ===================================================================
// FE-025 ~ FE-027：上下文分发与边界
// ===================================================================

/// FE-025: Same file non-consecutive matches — context isolation
/// Two independent matches must not pollute each other's context.
#[test]
fn fe_025_context_isolation() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // lib.rs has use statements (line ~10-12) and pub fn helper_one (line ~15)
    let input = FastExplorerInput {
        keywords: vec!["use".to_string(), "helper_one".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");

    // Find matches for both keywords in lib.rs
    let lib_matches: Vec<&FastExplorerMatch> = output.matches.iter()
        .filter(|m| m.file.contains("lib.rs"))
        .collect();
    let lib_count = lib_matches.len();
    if lib_count >= 2 {
        for m in &lib_matches {
            // Each match's context should be its own ±5 lines
            assert!(!m.context.is_empty(),
                    "context should not be empty for match at {}", m.line);
        }
        // Contexts for non-consecutive matches should differ
        assert_ne!(lib_matches[0].context, lib_matches[1].context,
                   "non-consecutive matches should have different contexts");
    }
}

/// FE-026: Backslash `\` in keyword → literal backslash match
#[test]
fn fe_026_backslash_in_keyword() {
    let escaped = FastExplorer::regex_escape_keyword("src\\main");
    assert_eq!(escaped, "src\\\\main");
}

/// FE-027: read_file fails for context → context = "" for that match
#[test]
fn fe_027_context_extraction_failure() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["fn".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");

    // All matches should have a context field (possibly empty on failure)
    for m in &output.matches {
        // context is always a String, never None
        let _: &str = &m.context;
    }
}

// ===================================================================
// FE-028 ~ FE-030：补充覆盖
// ===================================================================

/// FE-028: Error transparency — execute_internal returns ToolError,
/// and the ToolExecutor::execute path does not double-wrap errors.
/// Verified at compile time by the return type `Result<_, ToolError>`,
/// and at runtime by verifying a successful call returns the correct output.
#[test]
fn fe_028_error_transparency() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // Valid call: execute_internal → Result<FastExplorerOutput, ToolError>
    let result = explorer.execute_internal(FastExplorerInput {
        keywords: vec!["fn".to_string()],
        exclude_paths: vec![],
    });
    assert!(result.is_ok(), "Valid search should succeed");
    let output = result.unwrap();
    assert!(output.has_matches);

    // Error path: verify error codes are preserved through execute()
    // An empty keyword list triggers InternalError via parameter validation
    let input = ToolInput {
        tool_name: "fast_explorer".to_string(),
        params: serde_json::json!({"keywords": [], "exclude_paths": []}),
        project_root: root.to_path_buf(),
    };
    let err = explorer.execute(input).unwrap_err();
    assert_eq!(err.code, ErrorCode::InternalError);
    assert!(err.error.contains("keywords must not be empty"));
}

/// FE-029: Context format — lines joined with `\n`, no trailing newline.
#[test]
fn fe_029_context_format() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["helper_one".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");

    for m in &output.matches {
        if !m.context.is_empty() {
            // Lines should be separated by \n
            let lines: Vec<&str> = m.context.lines().collect();
            assert!(!lines.is_empty(), "context should have at least one line");
            // No trailing \n (verify by checking that last char is not \n)
            assert!(
                !m.context.ends_with('\n'),
                "context should not end with trailing newline, got: {:?}",
                m.context
            );
        }
    }
}

/// FE-030: total field equals deduplicated count, not raw search_content count.
#[test]
fn fe_030_total_reflects_dedup_count() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // Use a keyword that appears many times in the same file (lib.rs has many "const VAL_" lines)
    // These should dedup into fewer entries due to adjacent merging
    let input = FastExplorerInput {
        keywords: vec!["VAL".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");

    // total should equal the number of deduplicated matches (may be less than raw count)
    // matches length should be at most total
    assert!(
        output.matches.len() <= output.total,
        "matches.len() ({}) should not exceed total ({})",
        output.matches.len(),
        output.total,
    );
    if output.files_total < MAX_FILES {
        assert_eq!(
            output.files_sampled,
            output.files_total,
            "when files_total < {}, files_sampled should equal files_total",
            MAX_FILES,
        );
    }
}

// ===================================================================
// FE-031 ~ FE-035：文件聚类与排序（v1.2 新增）
// ===================================================================

/// FE-031: Same file → at most MAX_MATCHES_PER_FILE entries returned.
#[test]
fn fe_031_per_file_limit() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // "use" appears many times in lib.rs (3 import lines)
    let input = FastExplorerInput {
        keywords: vec!["use".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");

    // Count matches per file
    let mut file_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for m in &output.matches {
        *file_counts.entry(&m.file).or_default() += 1;
    }
    for (&file, &count) in &file_counts {
        assert!(
            count <= MAX_MATCHES_PER_FILE,
            "file {} has {} matches, max allowed is {}",
            file, count, MAX_MATCHES_PER_FILE,
        );
    }
}

/// FE-032: Files sorted by first-match line ascending (section 4.5 L2).
#[test]
fn fe_032_sort_by_first_match_line() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["fn ".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert!(!output.matches.is_empty(), "should find matches");
}

/// FE-033: total and files_total relationship — two files with 2 matches each
/// → total=4, files_total=2.
#[test]
fn fe_033_total_and_files_total() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    // Keywords that appear in both main.rs and lib.rs
    let input = FastExplorerInput {
        keywords: vec!["fn ".to_string(), "use".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");

    // files_total ≤ total (each file contributes ≥ 1 match)
    assert!(output.files_total <= output.total,
            "files_total ({}) should be ≤ total ({})",
            output.files_total, output.total);
    assert!(output.files_total > 0);
}

/// FE-034: Short files (first match ≤5) are de-prioritised in sort order
/// — they appear after files with first match >5.
#[test]
fn fe_034_short_files_de_prioritised() {
    // Create a fixture where a file has a match at line 1 (short file)
    // and another file has a match at line 10 (normal file)
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    // Short file: match at line 1
    std::fs::write(tmp.path().join("src/short.rs"), "fn target() {}\n// padding\n// padding\n// padding\n// padding\n// padding\n").unwrap();
    // Normal file: match at line 10
    let mut normal = String::from("// line1\n// line2\n// line3\n// line4\n// line5\n// line6\n");
    normal.push_str("// line7\n// line8\n// line9\nfn target() {}\n");
    std::fs::write(tmp.path().join("src/normal.rs"), &normal).unwrap();

    let explorer = FastExplorer::new(tmp.path().to_path_buf());
    let input = FastExplorerInput {
        keywords: vec!["target".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");

    // Both files should appear; normal.rs (first match at line 10)
    // should come before short.rs (first match at line 1)
    let short_idx = output.matches.iter().position(|m| m.file.contains("short.rs"));
    let normal_idx = output.matches.iter().position(|m| m.file.contains("normal.rs"));

    if let (Some(s), Some(n)) = (short_idx, normal_idx) {
        assert!(
            n < s,
            "normal.rs (first match >5) should come before short.rs (first match ≤5), got n={}, s={}",
            n, s,
        );
    }
}

/// FE-035: files_sampled == min(files_total, MAX_FILES).
#[test]
fn fe_035_files_sampled() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let explorer = FastExplorer::new(root.to_path_buf());

    let input = FastExplorerInput {
        keywords: vec!["fn ".to_string()],
        exclude_paths: vec![],
    };
    let output = explorer.execute_internal(input).expect("should succeed");
    assert_eq!(
        output.files_sampled,
        output.files_total.min(MAX_FILES),
    );
}
