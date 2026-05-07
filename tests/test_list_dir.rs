mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::list_dir::{ListDirTool, ListDirOutput, DirItem};

fn make_tool(root: &std::path::Path) -> ListDirTool {
    ListDirTool::new(root.to_path_buf())
}

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "list_dir".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

/// LD-001: List root directory
#[test]
fn ld_001_list_root() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "."
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    let names: Vec<&str> = output.items.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"src"));
    assert!(names.contains(&"tests"));
    assert!(names.contains(&"docs"));
}

/// LD-002: List subdirectory
#[test]
fn ld_002_list_subdirectory() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "src"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    let names: Vec<&str> = output.items.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"main.rs"));
    assert!(names.contains(&"lib.rs"));
    assert!(names.contains(&"utils"));
}

/// LD-003: Empty directory
#[test]
fn ld_003_empty_directory() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "empty_dir"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.items.is_empty());
}

/// LD-004: Sorting - directories first, then files
#[test]
fn ld_004_sort_dirs_first() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "."
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    let mut seen_file = false;
    for item in &output.items {
        if !item.is_dir {
            seen_file = true;
        }
        if item.is_dir && seen_file {
            panic!("Directory {:?} appeared after a file - dirs should come first", item.name);
        }
    }
}

/// LD-005: File sizes are correct, directory size = 0
#[test]
fn ld_005_file_sizes() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "src"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    for item in &output.items {
        if item.is_dir {
            assert_eq!(item.size, 0, "Directory {} should have size 0", item.name);
        } else {
            let actual_size = std::fs::metadata(root.join("src").join(&item.name))
                .map(|m| m.len())
                .unwrap_or(0);
            assert_eq!(item.size, actual_size, "Size mismatch for {}", item.name);
        }
    }
}

/// LD-006: Path traversal
#[test]
fn ld_006_path_traversal() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "../../"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathOutsideRoot);
}

/// LD-007: Path not found
#[test]
fn ld_007_path_not_found() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "nonexistent"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathNotFound);
}

/// LD-008: Path is a file
#[test]
fn ld_008_path_is_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "src/main.rs"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathNotDirectory);
}

/// LD-009: Non-recursive (direct children only)
#[test]
fn ld_009_non_recursive() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "src"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    // helper.py is in src/utils/, not directly in src/
    assert!(!output.items.iter().any(|i| i.name == "helper.py"));
}

/// LD-010: Path with spaces
#[test]
fn ld_010_path_with_spaces() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "special chars dir"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.items.iter().any(|i| i.name == "file with spaces.txt"));
}

/// LD-011: Hidden files are included
#[test]
fn ld_011_hidden_files_included() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "."
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.items.iter().any(|i| i.name == ".hidden_file"));
}

/// LD-012: Truncation at 1000 items
#[test]
fn ld_012_truncation() {
    let fixture = common::create_many_items_dir(1200);
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "path": "bigdir"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    assert_eq!(output.items.len(), 1000);
    assert!(output.truncated);
}

/// LD-013: Default path parameter
#[test]
fn ld_013_default_path() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({}));

    let result = tool.execute(input).expect("should succeed");
    let output: ListDirOutput = serde_json::from_value(result.data).unwrap();

    let names: Vec<&str> = output.items.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"src"));
}

// --- Unit test: sort_items ---

#[test]
fn sort_items_dirs_first_alpha_hidden_last() {
    let mut items = vec![
        DirItem { name: "zebra.txt".into(), is_dir: false, size: 10 },
        DirItem { name: "alpha".into(), is_dir: true, size: 0 },
        DirItem { name: ".hidden".into(), is_dir: false, size: 5 },
        DirItem { name: "beta.txt".into(), is_dir: false, size: 20 },
        DirItem { name: ".hidden_dir".into(), is_dir: true, size: 0 },
        DirItem { name: "gamma".into(), is_dir: true, size: 0 },
    ];

    ListDirTool::sort_items(&mut items);

    // Directories first (non-hidden alphabetical, then hidden)
    assert!(items[0].is_dir);
    assert!(items[1].is_dir);
    // Non-hidden dirs sorted: alpha, gamma
    // Hidden dirs: .hidden_dir
    // Then files: beta.txt, zebra.txt, .hidden

    let dir_items: Vec<&str> = items.iter().filter(|i| i.is_dir).map(|i| i.name.as_str()).collect();
    let file_items: Vec<&str> = items.iter().filter(|i| !i.is_dir).map(|i| i.name.as_str()).collect();

    assert_eq!(dir_items, vec!["alpha", "gamma", ".hidden_dir"]);
    assert_eq!(file_items, vec!["beta.txt", "zebra.txt", ".hidden"]);
}
