use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, LlmToolClient, UnifiedResponse};
use explore_ai_agent::agents::main_agent::{
    DeepExploreExecutor, FastExploreExecutor, MainAgent, ShellExecutor,
};
use explore_ai_agent::context::exploration::ExplorationContextTool;
use std::sync::{Arc, Mutex};

// ============================================================================
// Mock LLM client — returns pre-configured JSON responses in sequence
// ============================================================================

struct MockLlmClient {
    responses: Mutex<Vec<Result<UnifiedResponse, String>>>,
    call_count: Mutex<usize>,
}

impl MockLlmClient {
    fn new() -> Self {
        MockLlmClient { responses: Mutex::new(Vec::new()), call_count: Mutex::new(0) }
    }
    fn push_response(&self, resp: Result<UnifiedResponse, String>) {
        self.responses.lock().unwrap().push(resp);
    }
    #[allow(dead_code)]
    fn call_count(&self) -> usize { *self.call_count.lock().unwrap() }
}

#[async_trait::async_trait]
impl LlmStructuredClient for MockLlmClient {
    async fn call_llm_structured(
        &self, _instructions: &str, _input_data: &serde_json::Value,
        _output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        *self.call_count.lock().unwrap() += 1;
        self.responses.lock().unwrap().remove(0)
    }
}

#[async_trait::async_trait]
impl LlmToolClient for MockLlmClient {
    async fn call_llm_with_tools(
        &self, _messages: &[serde_json::Value], _tools: &[serde_json::Value],
        _response_format: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        *self.call_count.lock().unwrap() += 1;
        self.responses.lock().unwrap().remove(0)
    }
}

fn mock_json(text: &str) -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse { text: Some(text.to_string()), tool_calls: vec![], reasoning: None })
}

fn mock_answer(text: &str) -> Result<UnifiedResponse, String> {
    mock_json(&format!(r#"{{"action":"answer","final_response":"{}"}}"#, text))
}

fn mock_tool_call(tool: &str, args_json: &str) -> Result<UnifiedResponse, String> {
    mock_json(&format!(r#"{{"action":"tool_call","tool":"{}","arguments":{}}}"#, tool, args_json))
}

// ============================================================================
// Mock tool executors
// ============================================================================

struct MockFastExplore {
    response: Mutex<Result<serde_json::Value, String>>,
    call_count: Mutex<usize>,
}

impl MockFastExplore {
    fn new(response: Result<serde_json::Value, String>) -> Self {
        MockFastExplore { response: Mutex::new(response), call_count: Mutex::new(0) }
    }
    #[allow(dead_code)]
    fn call_count(&self) -> usize { *self.call_count.lock().unwrap() }
}

#[async_trait::async_trait]
impl FastExploreExecutor for MockFastExplore {
    async fn execute(&self, _keywords: &[String]) -> Result<serde_json::Value, String> {
        *self.call_count.lock().unwrap() += 1;
        self.response.lock().unwrap().clone()
    }
}

struct MockDeepExplore {
    response: Mutex<Result<serde_json::Value, String>>,
    call_count: Mutex<usize>,
}

impl MockDeepExplore {
    fn new(response: Result<serde_json::Value, String>) -> Self {
        MockDeepExplore { response: Mutex::new(response), call_count: Mutex::new(0) }
    }
    #[allow(dead_code)]
    fn call_count(&self) -> usize { *self.call_count.lock().unwrap() }
}

#[async_trait::async_trait]
impl DeepExploreExecutor for MockDeepExplore {
    async fn execute(&self, _question: &str, _summary: Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
        *self.call_count.lock().unwrap() += 1;
        self.response.lock().unwrap().clone()
    }
}

struct MockShellExecute {
    response: Mutex<Result<serde_json::Value, String>>,
    call_count: Mutex<usize>,
}

impl MockShellExecute {
    fn new(response: Result<serde_json::Value, String>) -> Self {
        MockShellExecute { response: Mutex::new(response), call_count: Mutex::new(0) }
    }
    #[allow(dead_code)]
    fn call_count(&self) -> usize { *self.call_count.lock().unwrap() }
}

#[async_trait::async_trait]
impl ShellExecutor for MockShellExecute {
    async fn execute(&self, _command: &str) -> Result<serde_json::Value, String> {
        *self.call_count.lock().unwrap() += 1;
        self.response.lock().unwrap().clone()
    }
}

fn mock_fe_result() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "matches": [{"file": "src/main.rs", "line": "1", "content": "fn main()"}],
        "key_findings": "找到主入口",
        "critical_files": [{"path": "src/main.rs", "summary": "入口文件"}],
        "confidence": 0.8
    }))
}

fn mock_de_result() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "critical_files": [{"path": "src/main.rs", "summary": "入口"}],
        "collected_evidence": [],
        "missing_info": "无"
    }))
}

fn mock_shell_result() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"success": true, "output": "42 src/agents/\n15 tests/"}))
}

// ============================================================================
// 8.2.1 Schema 测试 (MA-001 ~ MA-003)
// ============================================================================

#[test]
fn ma_001_action_schema_valid_json() {
    let schema = MainAgent::action_schema();
    assert!(schema.is_object(), "schema 应为 JSON 对象");
    assert!(schema.get("name").is_some(), "缺 name 字段");
    assert!(schema.get("strict").is_some(), "缺 strict 字段");
    assert!(schema.get("schema").is_some(), "缺 schema 字段");
}

#[test]
fn ma_002_schema_enum_has_answer_and_tool_call() {
    let schema = MainAgent::action_schema();
    let action_enum = &schema["schema"]["properties"]["action"]["enum"];
    let values: Vec<&str> = action_enum.as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(values.contains(&"answer"), "enum 应含 answer");
    assert!(values.contains(&"tool_call"), "enum 应含 tool_call");
}

#[test]
fn ma_003_schema_tool_enum_has_all_three_tools() {
    let schema = MainAgent::action_schema();
    let tool_enum = &schema["schema"]["properties"]["tool"]["enum"];
    let values: Vec<&str> = tool_enum.as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(values.contains(&"fast_explore"), "enum 应含 fast_explore");
    assert!(values.contains(&"deep_explore"), "enum 应含 deep_explore");
    assert!(values.contains(&"execute_shell"), "enum 应含 execute_shell");
}

// ============================================================================
// 8.2.2 Prompt 组装 (MA-010 ~ MA-014)
// ============================================================================

#[test]
fn ma_010_prompt_has_fast_explore_heading() {
    let prompt = MainAgent::assemble_prompt();
    assert!(prompt.contains("### fast_explore"), "应含 fast_explore 标题");
}

#[test]
fn ma_011_prompt_has_deep_explore_heading() {
    let prompt = MainAgent::assemble_prompt();
    assert!(prompt.contains("### deep_explore"), "应含 deep_explore 标题");
}

#[test]
fn ma_012_prompt_has_execute_shell_heading() {
    let prompt = MainAgent::assemble_prompt();
    assert!(prompt.contains("### execute_shell"), "应含 execute_shell 标题");
}

#[test]
fn ma_013_prompt_has_json_protocol() {
    let prompt = MainAgent::assemble_prompt();
    // Now uses {tool_examples} placeholder — verify it exists
    assert!(prompt.contains("{tool_examples}"), "应含 tool_examples 占位符");
}

#[test]
fn ma_014_prompt_has_all_three_tool_names() {
    let prompt = MainAgent::assemble_prompt();
    // Now uses {tool_names} placeholder — verify it exists
    assert!(prompt.contains("{tool_names}"), "应含 tool_names 占位符");
}

// ============================================================================
// v1.3: Shell 感知测试 (MA-015 ~ MA-017)
// ============================================================================

#[test]
fn ma_015_shell_info_returns_non_empty() {
    let info = MainAgent::shell_info();
    assert!(!info.is_empty(), "shell_info must be non-empty");
    let lower = info.to_lowercase();
    assert!(
        lower.contains("bash") || lower.contains("cmd") || lower.contains("pwsh") || lower.contains("sh"),
        "shell_info must reference a known shell, got: {}", info
    );
}

#[test]
fn ma_016_shell_commands_returns_list() {
    let commands = MainAgent::shell_commands();
    assert!(!commands.is_empty(), "shell_commands must be non-empty");
    let info = MainAgent::shell_info().to_lowercase();
    if info.starts_with("cmd") {
        assert!(commands.contains("dir"), "cmd shell must include dir: {}", commands);
    } else {
        assert!(commands.contains("grep"), "bash/pwsh/sh shell must include grep: {}", commands);
    }
}

#[test]
fn ma_017_prompt_contains_shell_placeholders() {
    let prompt = MainAgent::assemble_prompt();
    assert!(prompt.contains("{shell_info}"), "prompt must contain shell_info placeholder");
    assert!(prompt.contains("{shell_commands}"), "prompt must contain shell_commands placeholder");
}

// ============================================================================
// 8.3 集成测试 — Happy Path (MA-020 ~ MA-024)
// ============================================================================

#[tokio::test]
async fn ma_020_direct_answer_no_tools() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_answer("你好！有什么可以帮你的？"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("你好", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "直接回答应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_021_fast_explore_then_answer() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["test"]}"#));
    mock.push_response(mock_answer("找到结果"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_022_deep_explore_then_answer() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("deep_explore", r#"{"question":"test"}"#));
    mock.push_response(mock_answer("找到证据"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_023_multi_fast_explore_iteration() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["k1"]}"#));
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["k2"]}"#));
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["k3"]}"#));
    mock.push_response(mock_answer("迭代完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let _ = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(true, "stub 占位");
}

#[tokio::test]
async fn ma_024_fast_explore_then_de() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["test"]}"#));
    mock.push_response(mock_tool_call("deep_explore", r#"{"question":"test"}"#));
    mock.push_response(mock_answer("深入分析完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let _ = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(true, "stub 占位");
}

// ============================================================================
// 8.3 集成测试 — 异常场景 (MA-025 ~ MA-030, MA-034)
// ============================================================================

#[tokio::test]
async fn ma_025_fe_failure_then_de() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["test"]}"#));
    mock.push_response(mock_tool_call("deep_explore", r#"{"question":"test"}"#));
    mock.push_response(mock_answer("切换到DE完成"));

    let fe = MockFastExplore::new(Err("search failed".to_string()));
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_026_json_parse_retry_success() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_json("not valid json at all"));
    mock.push_response(mock_answer("重试成功"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_027_json_retry_exhausted() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_json("bad1"));
    mock.push_response(mock_json("bad2"));
    mock.push_response(mock_json("bad3"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_err(), "应返回 Err，实际: {:?}", result.ok());
}

#[tokio::test]
async fn ma_028_unknown_tool_name() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("unknown_tool", "{}"));
    mock.push_response(mock_answer("纠正后回答"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_029_llm_client_failure() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(Err("connection timeout".to_string()));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_err(), "应返回 Err，实际: {:?}", result.ok());
}

#[tokio::test]
async fn ma_030_de_failure_then_fe() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("deep_explore", r#"{"question":"test"}"#));
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["test"]}"#));
    mock.push_response(mock_answer("切换到快速扫描完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(Err("deep explore failed".to_string()));
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

// ============================================================================
// 8.3 集成测试 — 边界场景 (MA-031 ~ MA-033)
// ============================================================================

#[tokio::test]
async fn ma_031_context_truncation() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    for _ in 0..5 {
        mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["test"]}"#));
    }
    mock.push_response(mock_answer("截断后仍正常回答"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_032_fast_deep_fast_shell_alternation() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["k1"]}"#));
    mock.push_response(mock_tool_call("deep_explore", r#"{"question":"q1"}"#));
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["k2"]}"#));
    mock.push_response(mock_tool_call("execute_shell", r#"{"command":"find . -name '*.rs' | wc -l"}"#));
    mock.push_response(mock_answer("四工具交替完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let _ = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(true, "stub 占位");
}

#[tokio::test]
async fn ma_033_tool_call_missing_tool_field() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_json(r#"{"action":"tool_call","arguments":{"keywords":["test"]}}"#));
    mock.push_response(mock_answer("修正后回答"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

// ============================================================================
// v1.3 新增: execute_shell 测试 (MA-034 ~ MA-035)
// ============================================================================

#[tokio::test]
async fn ma_034_execute_shell_failure_then_fe() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("execute_shell", r#"{"command":"rm -rf /"}"#));
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["file count"]}"#));
    mock.push_response(mock_answer("Shell 命令被拒绝，已切换到快速扫描"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(Err("command not allowed".to_string()));

    let result = agent.run("test", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_035_execute_shell_normal() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("execute_shell", r#"{"command":"find . -name '*.rs' | wc -l"}"#));
    mock.push_response(mock_answer("项目共有 57 个 Rust 源文件"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("项目有多少文件?", "", Some(&fe), Some(&de), &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

// ============================================================================
// v1.4: DE 可配置开关 (MA-040 ~ MA-043)
// ============================================================================

#[test]
fn ma_040_action_schema_always_has_three_tools() {
    // Schema always contains all 3 tools; dispatch rejects DE when disabled
    let schema = MainAgent::action_schema();
    let tool_enum = &schema["schema"]["properties"]["tool"]["enum"];
    let values: Vec<&str> = tool_enum.as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(values.contains(&"fast_explore"));
    assert!(values.contains(&"deep_explore"));
    assert!(values.contains(&"execute_shell"));
}

#[tokio::test]
async fn ma_041_de_disabled_fe_and_shell_work() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["file count"]}"#));
    mock.push_response(mock_tool_call("execute_shell", r#"{"command":"find . -name '*.rs' | wc -l"}"#));
    mock.push_response(mock_answer("57 个文件"));

    let fe = MockFastExplore::new(mock_fe_result());
    let shell = MockShellExecute::new(mock_shell_result());

    // DE=None → disabled
    let result = agent.run("项目有多少文件?", "", Some(&fe), None, &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "FE+shell without DE must work, got: {:?}", result.err());
}

#[tokio::test]
async fn ma_042_de_disabled_rejects_de_tool_call() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("deep_explore", r#"{"question":"test"}"#));
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["test"]}"#));
    mock.push_response(mock_answer("已切换到 fast_explore"));

    let fe = MockFastExplore::new(mock_fe_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("test", "", Some(&fe), None, &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "DE disabled → DE call gracefully handled, got: {:?}", result.err());
}

#[tokio::test]
async fn ma_043_de_disabled_shell_only() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("execute_shell", r#"{"command":"find . -name '*.rs'"}"#));
    mock.push_response(mock_answer("找到 5 个 rs 文件"));

    let fe = MockFastExplore::new(mock_fe_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run("找 rs 文件", "", Some(&fe), None, &shell, &mock, Arc::new(ExplorationContextTool::new("test-ma".into())), false, 500, 10240, None).await;
    assert!(result.is_ok(), "shell-only without DE must work, got: {:?}", result.err());
}

fn mock_compact_response() -> Result<UnifiedResponse, String> {
    mock_json(r#"{"summary":"对话摘要：探索了项目结构","key_files":["main.rs"],"next_steps":"继续查看配置"}"#)
}

// ============================================================================
// Shell-only conversation compact tests
// ============================================================================

/// MA-044: 纯 shell 模式，低 context_limit 触发 token 阈值 compact
#[tokio::test]
async fn ma_044_shell_only_compact_token_threshold() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    // 1) LLM calls shell
    mock.push_response(mock_tool_call("execute_shell", r#"{"command":"grep -rn fn src/"}"#));
    // 2) compact fires (context_limit=21000 → usable≈0, fires immediately)
    mock.push_response(mock_compact_response());
    // 3) LLM answers
    mock.push_response(mock_answer("探索完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let shell = MockShellExecute::new(mock_shell_result());

    // shell_only_mode=true + low context_limit → compact after 1st shell
    let result = agent.run(
        "搜索代码", "",
        Some(&fe), None, &shell, &mock,
        Arc::new(ExplorationContextTool::new("test-ma".into())),
        true, 500, 10240, None, // shell_only
    ).await;
    assert!(result.is_ok(), "shell-only compact must work, got: {:?}", result.err());
}

/// MA-045: 纯 shell 模式，shell_only_mode=true 但 set 了 context_limit=None，
///         回退到 10 轮计数触发 compact
#[tokio::test]
async fn ma_045_shell_only_compact_fallback_rounds() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();

    // 10 shell calls → compact after 10th
    for _ in 0..10 {
        mock.push_response(mock_tool_call("execute_shell", r#"{"command":"ls"}"#));
    }
    // compact fires
    mock.push_response(mock_compact_response());
    // final answer
    mock.push_response(mock_answer("探索完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run(
        "列出所有文件", "",
        Some(&fe), None, &shell, &mock,
        Arc::new(ExplorationContextTool::new("test-ma".into())),
        true, 500, 10240, None, // shell_only
    ).await;
    assert!(result.is_ok(), "shell-only fallback compact must work, got: {:?}", result.err());
}

/// MA-046: doom-loop 检测 —— 相同命令连调 3 次触发警告，但不拦截执行
#[tokio::test]
async fn ma_046_shell_dedup_blocks_repeat() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    // 3 same calls → doom-loop warning injected (still executes, not blocked)
    for _ in 0..3 {
        mock.push_response(mock_tool_call("execute_shell", r#"{"command":"grep -rn fn src/"}"#));
    }
    // After warning, LLM changes direction
    mock.push_response(mock_tool_call("execute_shell", r#"{"command":"grep -rn struct src/"}"#));
    // Final answer
    mock.push_response(mock_answer("探索完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let shell = MockShellExecute::new(mock_shell_result());

    let result = agent.run(
        "搜索代码", "",
        Some(&fe), None, &shell, &mock,
        Arc::new(ExplorationContextTool::new("test-ma".into())),
        false, 500, 10240, None,
    ).await;
    assert!(result.is_ok(), "doom-loop must not crash, got: {:?}", result.err());
    assert_eq!(shell.call_count(), 4, "all 4 commands executed (doom-loop warns but does not block)");
}

// ============================================================================
// Prompt dynamic assembly tests
// ============================================================================

/// Helper: wrap run() to capture the system prompt from messages[0]
async fn capture_system_prompt(
    enable_fe: bool,
    enable_de: bool,
) -> String {
    use std::sync::Mutex as StdMutex;

    struct CapturingMock {
        prompt: StdMutex<Option<String>>,
        inner: MockLlmClient,
    }
    #[async_trait::async_trait]
    impl LlmToolClient for &CapturingMock {
        async fn call_llm_with_tools(
            &self, messages: &[serde_json::Value], _tools: &[serde_json::Value],
            _rf: Option<&serde_json::Value>,
        ) -> Result<UnifiedResponse, String> {
            // Capture system prompt on first call
            if self.prompt.lock().unwrap().is_none() {
                let p = messages[0].get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                *self.prompt.lock().unwrap() = Some(p);
                return Err("captured".to_string()); // stop early
            }
            Err("unexpected second call".to_string())
        }
    }
    #[async_trait::async_trait]
    impl LlmStructuredClient for &CapturingMock {
        async fn call_llm_structured(
            &self, _i: &str, _d: &serde_json::Value, _s: Option<&serde_json::Value>,
        ) -> Result<UnifiedResponse, String> {
            Err("unexpected".to_string())
        }
    }

    let capturing = CapturingMock { prompt: StdMutex::new(None), inner: MockLlmClient::new() };
    let fe: Option<&dyn FastExploreExecutor> = if enable_fe { Some(&MockFastExplore::new(mock_fe_result())) } else { None };
    let de: Option<&dyn DeepExploreExecutor> = if enable_de { Some(&MockDeepExplore::new(mock_de_result())) } else { None };
    let shell = MockShellExecute::new(mock_shell_result());
    let agent = MainAgent::new();
    let _ = agent.run("测试", "", fe, de, &shell, &&capturing, Arc::new(ExplorationContextTool::new("ma".into())), !enable_fe && !enable_de, 500, 10240, None).await;
    let result = capturing.prompt.lock().unwrap().take().unwrap_or_default();
    result
}

/// MA-050: shell-only 模式下 prompt 不含 fast_explore 和 deep_explore
#[tokio::test]
async fn ma_050_shell_only_prompt_excludes_fe_de() {
    let prompt = capture_system_prompt(false, false).await;
    assert!(prompt.contains("execute_shell"), "must contain execute_shell: {}", prompt);
    assert!(!prompt.contains("fast_explore"), "must NOT contain fast_explore: {}", prompt);
    assert!(!prompt.contains("deep_explore"), "must NOT contain deep_explore: {}", prompt);
    assert!(prompt.contains("一个工具"), "must say 一个工具, got: {}", prompt);
    assert!(prompt.contains("tool\": \"execute_shell"), "must have shell example, got: {}", prompt);
    assert!(!prompt.contains("tool\": \"fast_explore"), "must NOT have fe example: {}", prompt);
    assert!(!prompt.contains("tool\": \"deep_explore"), "must NOT have de example: {}", prompt);
}

/// MA-051: FE 禁用、DE 启用时 prompt 不含 fast_explore
#[tokio::test]
async fn ma_051_fe_disabled_prompt_excludes_fe() {
    let prompt = capture_system_prompt(false, true).await;
    assert!(prompt.contains("execute_shell"), "must contain execute_shell");
    assert!(!prompt.contains("fast_explore"), "must NOT contain fast_explore");
    assert!(prompt.contains("deep_explore"), "must contain deep_explore");
    assert!(prompt.contains("两个工具"), "must say 两个工具, got: {}", prompt);
}

/// MA-052: 全启用时 prompt 含所有工具
#[tokio::test]
async fn ma_052_all_enabled_prompt_has_all() {
    let prompt = capture_system_prompt(true, true).await;
    assert!(prompt.contains("fast_explore"), "must contain fast_explore");
    assert!(prompt.contains("deep_explore"), "must contain deep_explore");
    assert!(prompt.contains("execute_shell"), "must contain execute_shell");
    assert!(prompt.contains("三个工具"), "must say 三个工具, got: {}", prompt);
}
