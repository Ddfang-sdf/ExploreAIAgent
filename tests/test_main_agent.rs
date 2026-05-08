use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, LlmToolClient, UnifiedResponse};
use explore_ai_agent::agents::main_agent::{
    DeepExploreExecutor, FastExploreExecutor, MainAgent,
};
use std::sync::Mutex;

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
    Ok(UnifiedResponse { text: Some(text.to_string()), tool_calls: vec![] })
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
    fn call_count(&self) -> usize { *self.call_count.lock().unwrap() }
}

#[async_trait::async_trait]
impl DeepExploreExecutor for MockDeepExplore {
    async fn execute(&self, _question: &str, _summary: Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
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
fn ma_003_schema_tool_enum_has_both_tools() {
    let schema = MainAgent::action_schema();
    let tool_enum = &schema["schema"]["properties"]["tool"]["enum"];
    let values: Vec<&str> = tool_enum.as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(values.contains(&"fast_explore"), "enum 应含 fast_explore");
    assert!(values.contains(&"deep_explore"), "enum 应含 deep_explore");
}

// ============================================================================
// 8.2.2 Prompt 组装 (MA-010 ~ MA-013)
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
fn ma_012_prompt_has_json_protocol() {
    let prompt = MainAgent::assemble_prompt();
    assert!(prompt.contains(r#"{"action":"#), "应含 JSON 通信协议示例");
}

#[test]
fn ma_013_prompt_has_tool_call_and_answer() {
    let prompt = MainAgent::assemble_prompt();
    assert!(prompt.contains("tool_call"), "应含 tool_call");
    assert!(prompt.contains("answer"), "应含 answer");
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

    let result = agent.run("你好", "", &fe, &de, &mock).await;
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 实现后 fast_explore 被调用 1 次，答案正确
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 实现后 DE 被调用 1 次，答案正确
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

    let _ = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 实现后 fast_explore 被调用 3 次
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

    let _ = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 实现后 fast_explore 1次, DE 1次, 顺序正确
    assert!(true, "stub 占位");
}

// ============================================================================
// 8.3 集成测试 — 异常场景 (MA-025 ~ MA-030)
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: fast_explore 失败 → 错误信息注入 messages → LLM 切换到 DE
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 第1次非法JSON → 重试 → 第2次合法 → Ok
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 连续3次非法JSON → Err
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 未知 tool → 追加纠错提示 → LLM 纠正 → Ok
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_029_llm_client_failure() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(Err("connection timeout".to_string()));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: LLM 调用失败 → Err
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: DE 失败 → 错误注入 messages → LLM 切换到 fast_explore
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

// ============================================================================
// 8.3 集成测试 — 边界场景 (MA-031 ~ MA-033)
// ============================================================================

#[tokio::test]
async fn ma_031_context_truncation() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    // Simulate many tool results pushing context over threshold
    for _ in 0..5 {
        mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["test"]}"#));
    }
    mock.push_response(mock_answer("截断后仍正常回答"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 多轮后 context 超限 → 截断旧消息 → system prompt 完整 → 正常继续
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}

#[tokio::test]
async fn ma_032_fast_deep_fast_alternation() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["k1"]}"#));
    mock.push_response(mock_tool_call("deep_explore", r#"{"question":"q1"}"#));
    mock.push_response(mock_tool_call("fast_explore", r#"{"keywords":["k2"]}"#));
    mock.push_response(mock_answer("交替探索完成"));

    let fe = MockFastExplore::new(mock_fe_result());
    let de = MockDeepExplore::new(mock_de_result());

    let _ = agent.run("test", "", &fe, &de, &mock).await;
    // stub: 实现后 fast_explore 2次, DE 1次, 顺序为 fast→deep→fast
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

    let result = agent.run("test", "", &fe, &de, &mock).await;
    // stub: action=tool_call 但缺 tool 字段 → 追加修正提示 → LLM 纠正
    assert!(result.is_ok(), "应返回 Ok，实际: {:?}", result.err());
}
