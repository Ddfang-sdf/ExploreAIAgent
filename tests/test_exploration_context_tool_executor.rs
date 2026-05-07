use std::path::PathBuf;

use explore_ai_agent::common::models::{ToolInput, ToolOutput};
use explore_ai_agent::context::exploration::ExplorationContextTool;
use explore_ai_agent::tools::executor::ToolExecutor;

fn make_tool_input(params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "exploration_context_tool".to_string(),
        params,
        project_root: PathBuf::from("/tmp/test-project"),
    }
}

// TE-001: execute with write action (tool_call variant)
#[test]
fn exploration_context_tool_execute_write_tool_call() {
    let tool = ExplorationContextTool::new("session-te-001".to_string());
    let input = make_tool_input(serde_json::json!({
        "action": "write",
        "data": {
            "type": "tool_call",
            "source": "DeepExplorer",
            "tool": "read_file",
            "params": {"file": "src/main.rs", "lines": {"ranges": [[1, 10]]}},
            "result_summary": "Found main function at line 3",
            "confidence": 0.85
        }
    }));

    let result = tool.execute(input);
    assert!(result.is_ok());
    let output: ToolOutput = result.unwrap();
    assert!(output.success);
    assert!(output.data.get("record_id").is_some());
    assert!(output.data.get("total_records").is_some());
}

// TE-002: execute with write action (summary variant)
#[test]
fn exploration_context_tool_execute_write_summary() {
    let tool = ExplorationContextTool::new("session-te-002".to_string());
    let input = make_tool_input(serde_json::json!({
        "action": "write",
        "data": {
            "type": "summary",
            "source": "SearchStrategyAgent",
            "data": {
                "key_findings": "Found BooleanValidator.java",
                "critical_files": [
                    {"path": "src/validator.rs", "one_sentence_summary": "Contains validator logic"}
                ],
                "missing_info": "Missing config details",
                "confidence": 0.6
            }
        }
    }));

    let result = tool.execute(input);
    assert!(result.is_ok());
    let output: ToolOutput = result.unwrap();
    assert!(output.success);
    assert!(output.data.get("record_id").is_some());
}

// TE-003: execute with read action
#[test]
fn exploration_context_tool_execute_read() {
    let tool = ExplorationContextTool::new("session-te-003".to_string());
    let input = make_tool_input(serde_json::json!({
        "action": "read",
        "query": {
            "keyword": "main",
            "limit": 10
        }
    }));

    let result = tool.execute(input);
    assert!(result.is_ok());
    let output: ToolOutput = result.unwrap();
    assert!(output.success);
    assert!(output.data.get("records").is_some());
    assert!(output.data.get("total").is_some());
}

// TE-004: execute with unknown action returns INTERNAL_ERROR
#[test]
fn exploration_context_tool_execute_unknown_action() {
    let tool = ExplorationContextTool::new("session-te-004".to_string());
    let input = make_tool_input(serde_json::json!({
        "action": "delete",
        "data": {}
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code.to_string(), "INTERNAL_ERROR");
}

// TE-005: execute with write action missing data field returns INTERNAL_ERROR
#[test]
fn exploration_context_tool_execute_write_missing_data() {
    let tool = ExplorationContextTool::new("session-te-005".to_string());
    let input = make_tool_input(serde_json::json!({
        "action": "write"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code.to_string(), "INTERNAL_ERROR");
}

// TE-006: tool name and description are correct
#[test]
fn exploration_context_tool_name_and_description() {
    let tool = ExplorationContextTool::new("session-te-006".to_string());
    assert_eq!(tool.name(), "exploration_context_tool");
    assert!(!tool.description().is_empty());
}
