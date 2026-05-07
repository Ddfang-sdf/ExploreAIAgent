mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::search_content::{SearchContentTool, SearchContentOutput};

fn make_tool(root: &std::path::Path) -> SearchContentTool {
    SearchContentTool::new(root.to_path_buf())
}

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "search_content".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

/// SC-001: Basic keyword search
#[test]
fn sc_001_basic_keyword_search() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "fn main"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.matches.iter().any(|m| m.file == "src/main.rs"));
}

/// SC-002: Regex search
#[test]
fn sc_002_regex_search() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "fn\\s+\\w+"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(!output.matches.is_empty());
    // Should match function definitions in .rs files
    assert!(output.matches.iter().any(|m| m.content.contains("fn ")));
}

/// SC-003: Multiple keywords OR search
#[test]
fn sc_003_or_search() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "import|use|require"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    // Should find `use` in .rs files and `import` in .py/.java files
    assert!(output.matches.iter().any(|m|
        m.content.contains("use") || m.content.contains("import") || m.content.contains("require")
    ));
}

/// SC-004: File pattern filter
#[test]
fn sc_004_file_pattern_filter() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "def",
        "file_pattern": "*.py"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    // All matches should be in .py files
    for m in &output.matches {
        assert!(m.file.ends_with(".py"), "Expected .py file, got: {}", m.file);
    }
}

/// SC-005: Exclude paths
#[test]
fn sc_005_exclude_paths() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "config",
        "exclude_paths": ["docs/*"]
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    // No match from docs/
    assert!(!output.matches.iter().any(|m| m.file.starts_with("docs/")));
}

/// SC-006: Exclude test files (default)
#[test]
fn sc_006_exclude_test_files() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "assert",
        "exclude_test_files": true
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(!output.matches.iter().any(|m| m.file.starts_with("tests/")));
}

/// SC-007: Include test files
#[test]
fn sc_007_include_test_files() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "assert",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.matches.iter().any(|m| m.file.starts_with("tests/")));
}

/// SC-008: No matches
#[test]
fn sc_008_no_matches() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "zzz_nonexistent_pattern"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.matches.is_empty());
}

/// SC-009: Skip binary files
#[test]
fn sc_009_skip_binary_files() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": ".*",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(!output.matches.iter().any(|m| m.file.contains("binary_file.bin")));
}

/// SC-010: Skip .git directory
#[test]
fn sc_010_skip_git() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": ".*",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(!output.matches.iter().any(|m| m.file.contains(".git/")));
}

/// SC-011: Invalid regex
#[test]
fn sc_011_invalid_regex() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "(unclosed"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::InvalidPattern);
}

/// SC-012: Truncation at 500 matches
#[test]
fn sc_012_truncation() {
    let fixture = common::create_many_matches_fixture(100, 10);
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "MATCH_TARGET",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.matches.len(), 500);
    assert!(output.truncated);
}

/// SC-013: Case-sensitive matching
#[test]
fn sc_013_case_sensitive() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "Main"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    // On case-sensitive systems, "Main" should not match "main"
    for m in &output.matches {
        assert!(m.content.contains("Main"), "Expected case-sensitive match for 'Main', got: {}", m.content);
    }
}

/// SC-014: Skip files > 5 MB
#[test]
fn sc_014_skip_large_files() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "repeated line",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    // large_file.txt is > 5 MB and should be skipped
    assert!(!output.matches.iter().any(|m| m.file.contains("large_file.txt")));
}

/// SC-015: Multiple exclude_paths
#[test]
fn sc_015_multiple_exclude_paths() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "config",
        "exclude_paths": ["docs/*", "node_modules/*"]
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    for m in &output.matches {
        assert!(!m.file.starts_with("docs/"), "Should exclude docs/");
        assert!(!m.file.starts_with("node_modules/"), "Should exclude node_modules/");
    }
}

/// SC-016: context_lines basic functionality
#[test]
fn sc_016_context_lines() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "fn main",
        "context_lines": 2
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    for m in &output.matches {
        assert!(m.context_before.is_some(), "context_before should be present when context_lines > 0");
        assert!(m.context_after.is_some(), "context_after should be present when context_lines > 0");
        assert!(m.context_before.as_ref().unwrap().len() <= 2);
        assert!(m.context_after.as_ref().unwrap().len() <= 2);
    }
}

/// SC-017: context_lines = 0 (default) should not include context fields
#[test]
fn sc_017_context_lines_zero() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "fn main"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    for m in &output.matches {
        assert!(m.context_before.is_none(), "context_before should be None when context_lines = 0");
        assert!(m.context_after.is_none(), "context_after should be None when context_lines = 0");
    }
}

/// SC-018: context_lines exceeding max (5) should be clamped
#[test]
fn sc_018_context_lines_exceeds_max() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "fn main",
        "context_lines": 10
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    for m in &output.matches {
        // Should be clamped to 5
        assert!(m.context_before.as_ref().map_or(true, |v| v.len() <= 5));
        assert!(m.context_after.as_ref().map_or(true, |v| v.len() <= 5));
    }
}

/// SC-019: First line match has potentially empty context_before
#[test]
fn sc_019_first_line_context() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create a file where the first line matches
    std::fs::write(tmp.path().join("first_match.txt"), "MATCH_HERE at line 1\nline 2\nline 3\nline 4\nline 5\n").unwrap();
    let tool = make_tool(tmp.path());

    let input = make_input(tmp.path(), serde_json::json!({
        "pattern": "MATCH_HERE",
        "context_lines": 3,
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchContentOutput = serde_json::from_value(result.data).unwrap();

    assert!(!output.matches.is_empty());
    let first = &output.matches[0];
    // First line has no lines before it
    assert!(first.context_before.as_ref().map_or(true, |v| v.is_empty()));
    assert!(first.context_after.as_ref().map_or(false, |v| v.len() <= 3));
}
