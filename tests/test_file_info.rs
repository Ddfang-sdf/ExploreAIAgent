mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::file_info::*;

fn make_tool(root: &std::path::Path) -> FileInfoTool {
    FileInfoTool::new(root.to_path_buf())
}

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "file_info".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

/// FI-001: Code file basic info
#[test]
fn fi_001_code_file_basic() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert_eq!(output.file_type, FileType::Code);
    assert!(output.size > 0);
    assert_eq!(output.lines, 30);
}

/// FI-002: Code statistics accuracy
#[test]
fn fi_002_code_stats_accuracy() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.stats.is_some());
    let stats = output.stats.unwrap();
    assert_eq!(stats.comment_lines, 5, "Expected 5 comment lines, got {}", stats.comment_lines);
    assert_eq!(stats.blank_lines, 3, "Expected 3 blank lines, got {}", stats.blank_lines);
    assert_eq!(stats.lines_of_code, 42, "Expected 42 code lines, got {}", stats.lines_of_code);
    // Total should add up to file lines
    let total = stats.lines_of_code + stats.comment_lines + stats.blank_lines;
    assert_eq!(total, output.lines, "lines_of_code + comment_lines + blank_lines should equal total lines");
}

/// FI-003: Config file type
#[test]
fn fi_003_config_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/utils/config.yaml"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.file_type, FileType::Config);
    assert!(output.stats.is_none());
}

/// FI-004: Text file type
#[test]
fn fi_004_text_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "docs/readme.md"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.file_type, FileType::Text);
    assert!(output.stats.is_none());
}

/// FI-005: Directory type
#[test]
fn fi_005_directory() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.file_type, FileType::Directory);
    assert!(output.stats.is_none());
    assert!(output.header_comment.is_none());
}

/// FI-006: Binary file
#[test]
fn fi_006_binary_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "binary_file.bin"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.file_type, FileType::File);
    assert_eq!(output.lines, 0);
    assert!(output.stats.is_none());
}

/// FI-007: Header comment extraction (present)
#[test]
fn fi_007_header_comment_present() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // src/lib.rs has 5 lines of // comments at the top
    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.header_comment.is_some());
    let hc = output.header_comment.unwrap();
    assert!(hc.present);
    assert_eq!(hc.lines, 5);
    assert!(hc.content.contains("Library module"));
}

/// FI-008: Header comment not present
#[test]
fn fi_008_header_comment_absent() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // src/main.rs starts directly with code (use statement)
    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.header_comment.is_some());
    let hc = output.header_comment.unwrap();
    assert!(!hc.present);
}

/// FI-009: Python comment style
#[test]
fn fi_009_python_comments() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/utils/helper.py"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.file_type, FileType::Code);
    assert!(output.stats.is_some());
    let stats = output.stats.unwrap();
    // helper.py has # comments
    assert!(stats.comment_lines >= 2, "Expected Python # comments to be counted");
}

/// FI-010: Import statement counting
#[test]
fn fi_010_import_counting() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/lib.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    let stats = output.stats.unwrap();
    assert_eq!(stats.imports, 3, "lib.rs has 3 'use' statements");
}

/// FI-011: Function counting
#[test]
fn fi_011_function_counting() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    let stats = output.stats.unwrap();
    assert!(stats.functions >= 1, "main.rs has at least fn main");
}

/// FI-012: File not found
#[test]
fn fi_012_file_not_found() {
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

/// FI-013: Path traversal
#[test]
fn fi_013_path_traversal() {
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

/// FI-014: Unknown extension → type = "file"
#[test]
fn fi_014_unknown_extension() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": ".hidden_file"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.file_type, FileType::File);
}

/// FI-015: Header comment truncation at 20 lines
#[test]
fn fi_015_header_comment_truncation() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut content = String::new();
    for i in 1..=30 {
        content.push_str(&format!("// Comment line {}\n", i));
    }
    content.push_str("fn code_starts_here() {}\n");
    std::fs::write(tmp.path().join("many_comments.rs"), &content).unwrap();

    let tool = make_tool(tmp.path());
    let input = make_input(tmp.path(), serde_json::json!({
        "file": "many_comments.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.header_comment.is_some());
    let hc = output.header_comment.unwrap();
    assert_eq!(hc.lines, 20, "Header comment should be truncated to 20 lines");
}

/// FI-016: Multi-line comment block (/* ... */)
#[test]
fn fi_016_multiline_comment() {
    let tmp = tempfile::TempDir::new().unwrap();
    let content = "/*\n * Multi-line comment\n * Another line\n */\npublic class Test {\n    void method() {}\n}\n";
    std::fs::write(tmp.path().join("Test.java"), content).unwrap();

    let tool = make_tool(tmp.path());
    let input = make_input(tmp.path(), serde_json::json!({
        "file": "Test.java"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    let stats = output.stats.unwrap();
    assert!(stats.comment_lines >= 3, "Multi-line /* */ comment should count correctly, got {}", stats.comment_lines);
}

/// FI-017: Shebang with header comment
#[test]
fn fi_017_shebang_with_comment() {
    let tmp = tempfile::TempDir::new().unwrap();
    let content = "#!/usr/bin/env python3\n# Helper utilities\n# For testing\n# Third comment\nimport os\n";
    std::fs::write(tmp.path().join("script.py"), content).unwrap();

    let tool = make_tool(tmp.path());
    let input = make_input(tmp.path(), serde_json::json!({
        "file": "script.py"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.shebang, Some("#!/usr/bin/env python3".to_string()));
    assert!(output.header_comment.is_some());
    let hc = output.header_comment.unwrap();
    assert!(hc.present);
    assert_eq!(hc.lines, 3, "Shebang line should NOT be counted in header_comment lines");
}

/// FI-018: Shebang without subsequent comment
#[test]
fn fi_018_shebang_no_comment() {
    let tmp = tempfile::TempDir::new().unwrap();
    let content = "#!/bin/bash\necho \"Hello\"\n";
    std::fs::write(tmp.path().join("script.sh"), content).unwrap();

    let tool = make_tool(tmp.path());
    let input = make_input(tmp.path(), serde_json::json!({
        "file": "script.sh"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.shebang, Some("#!/bin/bash".to_string()));
    assert!(output.header_comment.is_some());
    assert!(!output.header_comment.unwrap().present);
}

/// FI-019: Non-script file has no shebang
#[test]
fn fi_019_no_shebang() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "file": "src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: FileInfoOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.shebang.is_none());
}

// --- Unit tests for file type detection ---

#[test]
fn file_type_detection() {
    assert_eq!(FileInfoTool::detect_file_type(Some("rs")), FileType::Code);
    assert_eq!(FileInfoTool::detect_file_type(Some("java")), FileType::Code);
    assert_eq!(FileInfoTool::detect_file_type(Some("py")), FileType::Code);
    assert_eq!(FileInfoTool::detect_file_type(Some("go")), FileType::Code);
    assert_eq!(FileInfoTool::detect_file_type(Some("js")), FileType::Code);
    assert_eq!(FileInfoTool::detect_file_type(Some("ts")), FileType::Code);
    assert_eq!(FileInfoTool::detect_file_type(Some("json")), FileType::Config);
    assert_eq!(FileInfoTool::detect_file_type(Some("yaml")), FileType::Config);
    assert_eq!(FileInfoTool::detect_file_type(Some("toml")), FileType::Config);
    assert_eq!(FileInfoTool::detect_file_type(Some("md")), FileType::Text);
    assert_eq!(FileInfoTool::detect_file_type(Some("txt")), FileType::Text);
    assert_eq!(FileInfoTool::detect_file_type(Some("xyz")), FileType::File);
    assert_eq!(FileInfoTool::detect_file_type(None), FileType::File);
}

// --- Verify all documented code extensions ---

#[test]
fn all_code_extensions_recognized() {
    for ext in CODE_EXTENSIONS {
        assert_eq!(
            FileInfoTool::detect_file_type(Some(ext)),
            FileType::Code,
            "Extension .{} should be recognized as Code",
            ext
        );
    }
}

#[test]
fn all_config_extensions_recognized() {
    for ext in CONFIG_EXTENSIONS {
        assert_eq!(
            FileInfoTool::detect_file_type(Some(ext)),
            FileType::Config,
            "Extension .{} should be recognized as Config",
            ext
        );
    }
}

#[test]
fn all_text_extensions_recognized() {
    for ext in TEXT_EXTENSIONS {
        assert_eq!(
            FileInfoTool::detect_file_type(Some(ext)),
            FileType::Text,
            "Extension .{} should be recognized as Text",
            ext
        );
    }
}
