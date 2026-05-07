mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::read_file::{ReadFileTool, ReadFileOutput, LineRanges};

fn make_tool(root: &std::path::Path) -> ReadFileTool {
    ReadFileTool::new(root.to_path_buf())
}

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "read_file".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

/// RF-001: Read complete file
#[test]
fn rf_001_read_complete_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.content.contains("fn main()"));
    assert_eq!(output.lines, "all");
    assert!(!output.truncated);
}

/// RF-002: Read specified line range
#[test]
fn rf_002_read_line_range() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs",
        "lines": {"ranges": [[10, 20]]}
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.lines, "10-20");
    let line_count = output.content.lines().count();
    assert_eq!(line_count, 11); // lines 10 through 20 inclusive
}

/// RF-003: Multiple line ranges
#[test]
fn rf_003_multiple_ranges() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs",
        "lines": {"ranges": [[1, 5], [10, 15]]}
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.lines, "1-5,10-15");
}

/// RF-004: Overlapping ranges should be merged
#[test]
fn rf_004_overlapping_ranges_merge() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs",
        "lines": {"ranges": [[1, 10], [5, 15]]}
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.lines, "1-15");
}

/// RF-005: Line range exceeding file actual lines - returns available portion
#[test]
fn rf_005_range_exceeds_file_lines() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // src/main.rs has 30 lines
    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs",
        "lines": {"ranges": [[25, 50]]}
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    let line_count = output.content.lines().count();
    assert!(line_count <= 6); // lines 25-30
}

/// RF-006: start > end should return INVALID_LINE_RANGE
#[test]
fn rf_006_start_greater_than_end() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs",
        "lines": {"ranges": [[20, 10]]}
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::InvalidLineRange);
}

/// RF-007: start <= 0 should return INVALID_LINE_RANGE
#[test]
fn rf_007_start_zero() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs",
        "lines": {"ranges": [[0, 5]]}
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::InvalidLineRange);
}

/// RF-008: File not found
#[test]
fn rf_008_file_not_found() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "nonexistent.rs"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathNotFound);
}

/// RF-009: Target is directory
#[test]
fn rf_009_target_is_directory() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathNotFile);
}

/// RF-010: Path traversal
#[test]
fn rf_010_path_traversal() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "../../../etc/passwd"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathOutsideRoot);
}

/// RF-011: Binary file detection
#[test]
fn rf_011_binary_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "binary_file.bin"
    }));

    let result = tool.execute(input).expect("should succeed (returns hint, not error)");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    // Content should be a hint message, not raw binary
    assert!(!output.content.contains('\0'));
}

/// RF-012: Large file without line range should return hint
#[test]
fn rf_012_large_file_no_range() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "large_file.txt"
    }));

    let result = tool.execute(input).expect("should succeed with hint");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    // Should suggest specifying a line range
}

/// RF-013: Large file with line range works normally
#[test]
fn rf_013_large_file_with_range() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "large_file.txt",
        "lines": {"ranges": [[1, 100]]}
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.content.lines().count(), 100);
}

/// RF-014: Truncation at 2000 lines
#[test]
fn rf_014_truncation_2000_lines() {
    let fixture = common::create_file_with_lines("huge.txt", 3000);
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "huge.txt"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.truncated);
    assert!(output.content.lines().count() <= 2000);
}

/// RF-015: Empty file
#[test]
fn rf_015_empty_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("empty.txt"), "").unwrap();
    let tool = make_tool(tmp.path());

    let input = make_input(tmp.path(), serde_json::json!({
        "file": "empty.txt"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.content, "");
    assert_eq!(output.lines, "all");
}

/// RF-016: File path with spaces
#[test]
fn rf_016_path_with_spaces() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "special chars dir/file with spaces.txt"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.content.contains("Content in file with spaces"));
}

/// RF-017: Non-UTF-8 encoding (lossy decode)
#[test]
fn rf_017_non_utf8_encoding() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Write some invalid UTF-8 bytes
    let data: Vec<u8> = vec![0xC4, 0xE3, 0xBA, 0xC3, 0x0A]; // GBK "你好\n"
    std::fs::write(tmp.path().join("gbk_file.txt"), &data).unwrap();
    let tool = make_tool(tmp.path());

    let input = make_input(tmp.path(), serde_json::json!({
        "file": "gbk_file.txt"
    }));

    let result = tool.execute(input).expect("should succeed with lossy decode");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    // Metadata should indicate lossy encoding
    assert!(result.metadata.as_ref()
        .and_then(|m| m.encoding_lossy)
        .unwrap_or(false));
}

/// RF-018: Lines parameter in string format "10-20"
#[test]
fn rf_018_string_format_lines() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs",
        "lines": "10-20"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.lines, "10-20");
}

/// RF-019: Lines parameter in string format with multiple ranges "1-5,10-15"
#[test]
fn rf_019_string_format_multi_ranges() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs",
        "lines": "1-5,10-15"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ReadFileOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.lines, "1-5,10-15");
}

// --- Unit tests for LineRanges parsing ---

#[test]
fn line_ranges_string_format_parsing() {
    let lr: LineRanges = serde_json::from_value(serde_json::json!("10-20")).unwrap();
    let ranges = lr.to_ranges().expect("should parse");
    assert_eq!(ranges, vec![(10, 20)]);
}

#[test]
fn line_ranges_structured_format_parsing() {
    let lr: LineRanges = serde_json::from_value(serde_json::json!({"ranges": [[1, 5], [10, 15]]})).unwrap();
    let ranges = lr.to_ranges().expect("should parse");
    assert_eq!(ranges, vec![(1, 5), (10, 15)]);
}

#[test]
fn line_ranges_multi_string_format() {
    let lr: LineRanges = serde_json::from_value(serde_json::json!("1-5,10-15")).unwrap();
    let ranges = lr.to_ranges().expect("should parse");
    assert_eq!(ranges, vec![(1, 5), (10, 15)]);
}

// --- Unit tests for merge_ranges ---

#[test]
fn merge_ranges_non_overlapping() {
    let mut ranges = vec![(1, 5), (10, 15)];
    let merged = ReadFileTool::merge_ranges(&mut ranges);
    assert_eq!(merged, vec![(1, 5), (10, 15)]);
}

#[test]
fn merge_ranges_overlapping() {
    let mut ranges = vec![(1, 10), (5, 15)];
    let merged = ReadFileTool::merge_ranges(&mut ranges);
    assert_eq!(merged, vec![(1, 15)]);
}

#[test]
fn merge_ranges_adjacent() {
    let mut ranges = vec![(1, 5), (6, 10)];
    let merged = ReadFileTool::merge_ranges(&mut ranges);
    assert_eq!(merged, vec![(1, 10)]);
}

// --- Unit tests for binary detection ---

#[test]
fn binary_detection_with_nul_bytes() {
    let mut data = vec![0u8; 100];
    data[50] = 0x00; // NUL byte
    assert!(ReadFileTool::is_binary_file(&data));
}

#[test]
fn binary_detection_clean_text() {
    let data = b"Hello, world!\nThis is a text file.\n";
    assert!(!ReadFileTool::is_binary_file(data));
}
