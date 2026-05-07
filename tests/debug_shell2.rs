mod common;

use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::execute_shell::{ExecuteShellTool, ExecuteShellOutput};
use explore_ai_agent::common::path_manager::PathManager;

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
fn debug_es_040_path() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    
    // Check what PathManager resolves "src" to
    let pm = PathManager::new(root.to_path_buf());
    let resolved = pm.validate("src").unwrap();
    eprintln!("=== DEBUG PATH ===");
    eprintln!("root: {:?}", root);
    eprintln!("root canonicalized: {:?}", root.canonicalize().unwrap());
    eprintln!("resolved 'src': {:?}", resolved);
    
    // Check src dir contents
    let entries: Vec<_> = std::fs::read_dir(&resolved).unwrap().map(|e| e.unwrap().file_name()).collect();
    eprintln!("src dir entries: {:?}", entries);
    
    // Run ls with the resolved path directly through FFI
    let resolved_str = resolved.to_string_lossy().to_string();
    eprintln!("resolved_str: {}", resolved_str);
    
    let tool = make_tool(root);
    let input = make_input(root, serde_json::json!({
        "command": "ls",
        "working_dir": "src"
    }));
    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();
    eprintln!("ls output first 200 chars: {:?}", &output.output[..std::cmp::min(200, output.output.len())]);
}
