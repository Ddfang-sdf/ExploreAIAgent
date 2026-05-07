mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::tools::registry::ToolRegistry;
use explore_ai_agent::tools::search_content::SearchContentOutput;
use explore_ai_agent::tools::search_files::SearchFilesOutput;
use explore_ai_agent::tools::read_file::ReadFileOutput;
use explore_ai_agent::tools::file_info::FileInfoOutput;

/// AT-001: Fast exploration chain: search → read
/// SearchStrategyAgent would: search_content → get file and line → read_file for context
#[test]
fn at_001_search_then_read() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let registry = ToolRegistry::new(root.to_path_buf());

    // Step 1: search_content for "fn main"
    let search_result = registry.execute("search_content", serde_json::json!({
        "pattern": "fn main"
    })).expect("search should succeed");
    let search_output: SearchContentOutput = serde_json::from_value(search_result.data).unwrap();

    assert!(!search_output.matches.is_empty(), "Should find fn main");

    let first_match = &search_output.matches[0];
    let file_path = &first_match.file;
    let match_line = first_match.line;

    // Step 2: read_file with line range around the match
    let start = if match_line > 10 { match_line - 10 } else { 1 };
    let end = match_line + 10;

    let read_result = registry.execute("read_file", serde_json::json!({
        "file": file_path,
        "lines": {"ranges": [[start, end]]}
    })).expect("read should succeed");
    let read_output: ReadFileOutput = serde_json::from_value(read_result.data).unwrap();

    assert!(read_output.success);
    assert!(read_output.content.contains("fn main"));
}

/// AT-002: Deep exploration chain: search_files → file_info → read_file
#[test]
fn at_002_search_files_info_read() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let registry = ToolRegistry::new(root.to_path_buf());

    // Step 1: search_files for .rs files
    let search_result = registry.execute("search_files", serde_json::json!({
        "pattern": "**/*.rs"
    })).expect("search_files should succeed");
    let search_output: SearchFilesOutput = serde_json::from_value(search_result.data).unwrap();

    assert!(!search_output.files.is_empty());
    let first_file = &search_output.files[0];

    // Step 2: file_info on the first file
    let info_result = registry.execute("file_info", serde_json::json!({
        "file": first_file
    })).expect("file_info should succeed");
    let info_output: FileInfoOutput = serde_json::from_value(info_result.data).unwrap();

    assert!(info_output.success);
    let total_lines = info_output.lines;

    // Step 3: read_file with lines "1-50"
    let read_end = std::cmp::min(50, total_lines);
    let read_result = registry.execute("read_file", serde_json::json!({
        "file": first_file,
        "lines": format!("1-{}", read_end)
    })).expect("read_file should succeed");
    let read_output: ReadFileOutput = serde_json::from_value(read_result.data).unwrap();

    assert!(read_output.success);
    assert!(read_output.content.lines().count() <= read_end as usize);
}

/// AT-003: Error recovery - PATH_NOT_FOUND then retry with correct path
#[test]
fn at_003_error_recovery() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let registry = ToolRegistry::new(root.to_path_buf());

    // Step 1: Attempt read of nonexistent file
    let err_result = registry.execute("read_file", serde_json::json!({
        "file": "src/nonexistent.rs"
    }));

    assert!(err_result.is_err());
    let err = err_result.unwrap_err();

    // Step 2: Verify error code is machine-readable
    assert_eq!(err.code, ErrorCode::PathNotFound);
    assert_eq!(err.code.as_str(), "PATH_NOT_FOUND");

    // Step 3: Search for correct file
    let search_result = registry.execute("search_files", serde_json::json!({
        "pattern": "**/*.rs"
    })).expect("search should succeed");
    let search_output: SearchFilesOutput = serde_json::from_value(search_result.data).unwrap();

    assert!(!search_output.files.is_empty());

    // Step 4: Retry with correct path
    let read_result = registry.execute("read_file", serde_json::json!({
        "file": &search_output.files[0]
    })).expect("retry read should succeed");
    let read_output: ReadFileOutput = serde_json::from_value(read_result.data).unwrap();

    assert!(read_output.success);
}

/// AT-004: Error code machine-readability verification
#[test]
fn at_004_error_codes_machine_readable() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let registry = ToolRegistry::new(root.to_path_buf());

    // Trigger each error and verify its code string
    let test_cases: Vec<(&str, serde_json::Value, ErrorCode)> = vec![
        ("read_file", serde_json::json!({"file": "../../../etc/passwd"}), ErrorCode::PathOutsideRoot),
        ("read_file", serde_json::json!({"file": "nonexistent.rs"}), ErrorCode::PathNotFound),
        ("read_file", serde_json::json!({"file": "src"}), ErrorCode::PathNotFile),
        ("list_dir", serde_json::json!({"path": "src/main.rs"}), ErrorCode::PathNotDirectory),
        ("search_files", serde_json::json!({"pattern": "**/*.rs["}), ErrorCode::InvalidPattern),
        ("search_content", serde_json::json!({"pattern": "(unclosed"}), ErrorCode::InvalidPattern),
        ("read_file", serde_json::json!({"file": "src/main.rs", "lines": {"ranges": [[20, 10]]}}), ErrorCode::InvalidLineRange),
        ("execute_shell", serde_json::json!({"command": "rm -rf /"}), ErrorCode::ShellCmdNotAllowed),
        ("execute_shell", serde_json::json!({"command": "ls > out.txt"}), ErrorCode::ShellDangerousOperator),
    ];

    for (tool_name, params, expected_code) in test_cases {
        let result = registry.execute(tool_name, params);
        assert!(result.is_err(), "Expected error for {} with {:?}", tool_name, expected_code);
        let err = result.unwrap_err();
        assert_eq!(err.code, expected_code,
            "Tool '{}' expected code {:?}, got {:?}", tool_name, expected_code, err.code);

        // Verify string representation is valid
        let code_str = err.code.as_str();
        let round_trip = ErrorCode::from_str(code_str);
        assert_eq!(round_trip, Some(expected_code.clone()),
            "ErrorCode round-trip failed for {}", code_str);
    }
}

/// AT-005: Tool output JSON serialization consistency
#[test]
fn at_005_json_serialization() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let registry = ToolRegistry::new(root.to_path_buf());

    let tools_and_params: Vec<(&str, serde_json::Value)> = vec![
        ("search_files", serde_json::json!({"pattern": "**/*.rs"})),
        ("read_file", serde_json::json!({"file": "src/main.rs"})),
        ("search_content", serde_json::json!({"pattern": "fn"})),
        ("list_dir", serde_json::json!({"path": "."})),
        ("file_info", serde_json::json!({"file": "src/main.rs"})),
        ("execute_shell", serde_json::json!({"command": "ls"})),
    ];

    for (tool_name, params) in tools_and_params {
        let result = registry.execute(tool_name, params);
        assert!(result.is_ok(), "Tool '{}' should succeed", tool_name);

        let output = result.unwrap();
        assert!(output.success);

        // Verify data is valid JSON that can be re-serialized
        let serialized = serde_json::to_string(&output.data);
        assert!(serialized.is_ok(), "Tool '{}' output should serialize to JSON", tool_name);

        // Verify it can be deserialized back
        let deserialized: Result<serde_json::Value, _> = serde_json::from_str(&serialized.unwrap());
        assert!(deserialized.is_ok(), "Tool '{}' output JSON round-trip should work", tool_name);
    }
}

/// Verify all 6 tools are registered
#[test]
fn registry_has_all_tools() {
    let tmp = tempfile::TempDir::new().unwrap();
    let registry = ToolRegistry::new(tmp.path().to_path_buf());

    let tools = registry.list_tools();
    let expected = vec!["search_files", "read_file", "search_content", "list_dir", "file_info", "execute_shell"];

    for name in expected {
        assert!(tools.contains(&name), "Registry should contain tool '{}'", name);
    }
}
