mod common;

use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::execute_shell::{ExecuteShellTool, ExecuteShellOutput};

fn make_tool(root: &std::path::Path) -> ExecuteShellTool {
    ExecuteShellTool::new(root.to_path_buf())
}

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "execute_shell".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

#[test]
fn debug_es_040() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls",
        "working_dir": "src"
    }));

    let result = tool.execute(input);
    match result {
        Ok(output) => {
            let parsed: ExecuteShellOutput = serde_json::from_value(output.data).unwrap();
            eprintln!("=== ES-040 DEBUG ===");
            eprintln!("success: {}", parsed.success);
            eprintln!("output: {:?}", parsed.output);
            eprintln!("error: {:?}", parsed.error);
        }
        Err(e) => {
            eprintln!("=== ES-040 ERROR ===");
            eprintln!("code: {:?}", e.code);
            eprintln!("error: {}", e.error);
        }
    }
}

#[test]
fn debug_es_050() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "grep -rn config src/",
        "exclude_paths": ["vendor/*"]
    }));

    let result = tool.execute(input);
    match result {
        Ok(output) => {
            let parsed: ExecuteShellOutput = serde_json::from_value(output.data).unwrap();
            eprintln!("=== ES-050 DEBUG ===");
            eprintln!("success: {}", parsed.success);
            eprintln!("output: {:?}", parsed.output);
            eprintln!("error: {:?}", parsed.error);
        }
        Err(e) => {
            eprintln!("=== ES-050 ERROR ===");
            eprintln!("code: {:?}", e.code);
            eprintln!("error: {}", e.error);
        }
    }
}

#[test]
fn debug_es_043() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // Check large_file exists
    let large_file = root.join("large_file.txt");
    let metadata = std::fs::metadata(&large_file).unwrap();
    eprintln!("=== ES-043 DEBUG ===");
    eprintln!("large_file size: {} bytes", metadata.len());

    let input = make_input(root, serde_json::json!({
        "command": "cat large_file.txt"
    }));

    let result = tool.execute(input);
    match result {
        Ok(output) => {
            let parsed: ExecuteShellOutput = serde_json::from_value(output.data).unwrap();
            eprintln!("success: {}", parsed.success);
            eprintln!("output len: {}", parsed.output.len());
            eprintln!("truncated: {}", output.truncated);
        }
        Err(e) => {
            eprintln!("ERROR code: {:?}", e.code);
            eprintln!("ERROR msg: {}", e.error);
        }
    }
}
