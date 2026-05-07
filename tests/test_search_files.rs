mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::search_files::{SearchFilesOutput, SearchFilesTool};

fn make_tool(root: &std::path::Path) -> SearchFilesTool {
    SearchFilesTool::new(root.to_path_buf())
}

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "search_files".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

/// SF-001: Basic glob matching - **/*.rs should find .rs files in src/
#[test]
fn sf_001_basic_glob_match() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.rs",
        "path": "."
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.files.contains(&"src/main.rs".to_string()));
    assert!(output.files.contains(&"src/lib.rs".to_string()));
    // Default exclude_test_files=true, so tests/ .rs should NOT be present
    assert!(!output.files.iter().any(|f| f.starts_with("tests/")));
}

/// SF-002: Search in subdirectory
#[test]
fn sf_002_subdirectory_search() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "*.py",
        "path": "src/utils"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.files.contains(&"src/utils/helper.py".to_string()));
}

/// SF-003: Exclude test files (default behavior)
#[test]
fn sf_003_exclude_test_files_default() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.rs",
        "exclude_test_files": true
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(!output.files.contains(&"tests/test_main.rs".to_string()));
}

/// SF-004: Include test files when exclude_test_files=false
#[test]
fn sf_004_include_test_files() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.rs",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.files.contains(&"tests/test_main.rs".to_string()));
}

/// SF-005: No matches returns empty array, success=true
#[test]
fn sf_005_no_matches() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.xyz"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.files.is_empty());
    assert!(output.success);
}

/// SF-006: .git directory should be skipped
#[test]
fn sf_006_skip_git_directory() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(!output.files.iter().any(|f| f.contains(".git/")));
}

/// SF-007: node_modules should be skipped
#[test]
fn sf_007_skip_node_modules() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.js",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(!output.files.iter().any(|f| f.contains("node_modules/")));
}

/// SF-008: Invalid glob syntax should return INVALID_PATTERN
#[test]
fn sf_008_invalid_glob_syntax() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.rs["
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidPattern);
}

/// SF-009: Path traversal should return PATH_OUTSIDE_ROOT
#[test]
fn sf_009_path_traversal() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "*.rs",
        "path": "../../etc"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::PathOutsideRoot);
}

/// SF-010: Nonexistent path should return PATH_NOT_FOUND
#[test]
fn sf_010_path_not_found() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "*.rs",
        "path": "nonexistent"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::PathNotFound);
}

/// SF-011: Path is file, not directory
#[test]
fn sf_011_path_is_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "*.rs",
        "path": "src/main.rs"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::PathNotDirectory);
}

/// SF-012: Truncation when results exceed 1000
#[test]
fn sf_012_truncation() {
    let fixture = common::create_many_files_fixture(1200, "txt");
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.txt",
        "exclude_test_files": false
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.files.len(), 1000);
    assert!(output.truncated);
}

/// SF-013: Empty directory returns empty results
#[test]
fn sf_013_empty_directory() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*",
        "path": "empty_dir"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.files.is_empty());
}

/// SF-014: Paths with spaces
#[test]
fn sf_014_path_with_spaces() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "*.txt",
        "path": "special chars dir"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.files.iter().any(|f| f.contains("file with spaces.txt")));
}

/// SF-015: Default path parameter (omitted) should behave like "."
#[test]
fn sf_015_default_path() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "pattern": "**/*.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: SearchFilesOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.files.contains(&"src/main.rs".to_string()));
}

// --- Test helper: is_test_file detection ---

#[test]
fn test_file_detection_by_directory() {
    assert!(SearchFilesTool::is_test_file("test/foo.rs"));
    assert!(SearchFilesTool::is_test_file("tests/bar.py"));
    assert!(SearchFilesTool::is_test_file("__tests__/baz.js"));
    assert!(SearchFilesTool::is_test_file("__test__/qux.ts"));
    assert!(SearchFilesTool::is_test_file("spec/hello.rb"));
    assert!(SearchFilesTool::is_test_file("specs/world.rb"));
    assert!(!SearchFilesTool::is_test_file("src/main.rs"));
}

#[test]
fn test_file_detection_by_filename() {
    assert!(SearchFilesTool::is_test_file("src/foo_test.rs"));
    assert!(SearchFilesTool::is_test_file("src/foo_spec.js"));
    assert!(SearchFilesTool::is_test_file("src/test_foo.py"));
    assert!(SearchFilesTool::is_test_file("src/FooTest.java"));
    assert!(SearchFilesTool::is_test_file("src/FooSpec.scala"));
    assert!(!SearchFilesTool::is_test_file("src/foo.rs"));
}

#[test]
fn skipped_directories() {
    assert!(SearchFilesTool::is_skipped_directory(".git"));
    assert!(SearchFilesTool::is_skipped_directory("node_modules"));
    assert!(SearchFilesTool::is_skipped_directory("target"));
    assert!(SearchFilesTool::is_skipped_directory(".idea"));
    assert!(SearchFilesTool::is_skipped_directory("__pycache__"));
    assert!(SearchFilesTool::is_skipped_directory("vendor"));
    assert!(SearchFilesTool::is_skipped_directory("build"));
    assert!(SearchFilesTool::is_skipped_directory("dist"));
    assert!(SearchFilesTool::is_skipped_directory(".svn"));
    assert!(SearchFilesTool::is_skipped_directory(".hg"));
    assert!(SearchFilesTool::is_skipped_directory(".vscode"));
    assert!(SearchFilesTool::is_skipped_directory(".tox"));
    assert!(!SearchFilesTool::is_skipped_directory("src"));
    assert!(!SearchFilesTool::is_skipped_directory("lib"));
}
