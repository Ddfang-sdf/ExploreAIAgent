use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, UnifiedResponse};
use explore_ai_agent::agents::tool_result_refiner::*;
use std::sync::Mutex;

// ============================================================================
// Mock LLM client
// ============================================================================

struct MockLlmClient {
    response: Mutex<Option<Result<UnifiedResponse, String>>>,
}

impl MockLlmClient {
    fn new() -> Self {
        MockLlmClient { response: Mutex::new(None) }
    }
    fn set_response(&self, r: Result<UnifiedResponse, String>) {
        *self.response.lock().unwrap() = Some(r);
    }
}

#[async_trait::async_trait]
impl LlmStructuredClient for MockLlmClient {
    async fn call_llm_structured(
        &self,
        _instructions: &str,
        _input_data: &serde_json::Value,
        _output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        self.response.lock().unwrap().take().expect("Mock called without preset response")
    }
}

// ============================================================================
// helpers
// ============================================================================

fn mock_refined_response(summary: &str) -> Result<UnifiedResponse, String> {
    let json = format!(r#"{{"summary":"{}"}}"#, summary);
    Ok(UnifiedResponse { text: Some(json), tool_calls: vec![], reasoning: None })
}

fn mock_pure_text(text: &str) -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse { text: Some(text.to_string()), tool_calls: vec![], reasoning: None })
}

fn make_search_content_result() -> serde_json::Value {
    serde_json::json!({
        "matches": [
            {"file": "src/backtest/engine.py", "line": "142", "content": "def run_backtest(self, start_date, end_date):"},
            {"file": "src/backtest/engine.py", "line": "215", "content": "results = self.calculate_metrics()"},
            {"file": "app/backend/services/backtest_service.py", "line": "30", "content": "class BacktestService:"}
        ],
        "total_matches": 15,
    })
}

fn make_read_file_result() -> serde_json::Value {
    serde_json::json!({
        "content": "class BacktestEngine:\n    def run_backtest(self, start, end):\n        ...\n    def calculate_metrics(self):\n        ...",
        "lines": "1-20",
    })
}

fn make_search_files_result() -> serde_json::Value {
    serde_json::json!({
        "files": ["src/backtest/engine.py", "src/backtest/config.py", "app/backend/services/backtest_service.py"],
    })
}

fn make_list_dir_result() -> serde_json::Value {
    serde_json::json!({
        "items": [
            {"name": "engine.py", "is_dir": false, "size": 4500},
            {"name": "config.py", "is_dir": false, "size": 1200},
            {"name": "tests", "is_dir": true, "size": 0}
        ],
    })
}

fn make_empty_search_result() -> serde_json::Value {
    serde_json::json!({"matches": [], "total": 0})
}

// ============================================================================
// 7.2 自动化单元测试
// ============================================================================

#[test]
fn tr_001_schema_returns_valid_json() {
    // 输入: output_schema()
    // output_schema() → TOOL_RESULT_REFINER_SCHEMA 静态字符串 → from_str 解析
    let schema = ToolResultRefinerAgent::output_schema();
    let parsed: serde_json::Value = serde_json::from_str(schema).expect("Schema must be valid JSON");
    assert!(parsed.get("name").and_then(|v| v.as_str()).is_some());
    assert!(parsed.get("strict").and_then(|v| v.as_bool()).unwrap_or(false));
    assert!(parsed.get("schema").is_some());
}

#[test]
fn tr_002_schema_required_contains_only_summary() {
    // 输入: output_schema()
    // output_schema() → JSON → schema.required 数组
    let schema = ToolResultRefinerAgent::output_schema();
    let parsed: serde_json::Value = serde_json::from_str(schema).unwrap();
    let required = parsed
        .get("schema")
        .and_then(|s| s.get("required"))
        .and_then(|r| r.as_array())
        .expect("required must be an array");
    let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(names, vec!["summary"],
        "required must contain ONLY 'summary'");
}

#[test]
fn tr_003_prompt_contains_refinement_rules() {
    // 输入: assemble_instructions()
    // assemble_instructions() → 返回 Prompt 模板字符串
    let instructions = ToolResultRefinerAgent::assemble_instructions();
    assert!(instructions.contains("{question}"), "must contain question placeholder");
    assert!(instructions.contains("{tool_name}"), "must contain tool_name placeholder");
    assert!(instructions.contains("{tool_result}"), "must contain tool_result placeholder");
    assert!(instructions.contains("可操作实体"), "must contain '可操作实体' keyword");
}

// ============================================================================
// 7.3.1 正常提炼 — 验证忠实性
// ============================================================================

#[tokio::test]
async fn tr_010_search_content_refinement() {
    // 输入: question="回测怎么实现?" tool_name="search_content"
    //       tool_result=make_search_content_result()
    //       mock LLM → UnifiedResponse{text: {"summary":"匹配到..."}}
    // refine() → call_llm_structured → from_str → RefinedToolResult{summary}
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    let tool_result = make_search_content_result();
    let mock_summary = "匹配到 15 个文件：src/backtest/engine.py (line 142: def run_backtest...)";
    client.set_response(mock_refined_response(mock_summary));

    let result = tr.refine("回测怎么实现?", "search_content", &tool_result, &client).await;
    assert!(result.is_ok(), "refine must return Ok");
    let refined = result.unwrap();
    assert!(refined.summary.contains("src/backtest/engine.py"),
        "summary must contain actual file path from tool_result");
    assert!(refined.summary.contains("142"),
        "summary must contain actual line number from tool_result");
}

#[tokio::test]
async fn tr_011_read_file_refinement() {
    // 输入: question="回测怎么实现?" tool_name="read_file"
    //       tool_result=make_read_file_result()
    //       mock LLM → UnifiedResponse{text: {"summary":"类 BacktestEngine..."}}
    // refine() → call_llm_structured → from_str → RefinedToolResult{summary}
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    let tool_result = make_read_file_result();
    let mock_summary = "类 BacktestEngine 含方法 run_backtest、calculate_metrics";
    client.set_response(mock_refined_response(mock_summary));

    let result = tr.refine("回测怎么实现?", "read_file", &tool_result, &client).await;
    assert!(result.is_ok(), "refine must return Ok");
    let refined = result.unwrap();
    assert!(refined.summary.contains("run_backtest") || refined.summary.contains("BacktestEngine"),
        "summary must contain actual function/class name from input file");
}

#[tokio::test]
async fn tr_012_search_files_refinement() {
    // 输入: question="回测怎么实现?" tool_name="search_files"
    //       tool_result=make_search_files_result()
    //       mock LLM → UnifiedResponse{text: {"summary":"找到 3 个文件：..."}}
    // refine() → call_llm_structured → from_str → RefinedToolResult{summary}
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    let tool_result = make_search_files_result();
    let mock_summary = "找到 3 个文件：src/backtest/engine.py, src/backtest/config.py, app/backend/services/backtest_service.py";
    client.set_response(mock_refined_response(mock_summary));

    let result = tr.refine("回测怎么实现?", "search_files", &tool_result, &client).await;
    assert!(result.is_ok(), "refine must return Ok");
    let refined = result.unwrap();
    assert!(refined.summary.contains("src/backtest/engine.py"),
        "summary must contain actual file path from tool_result");
}

#[tokio::test]
async fn tr_013_list_dir_refinement() {
    // 输入: question="回测怎么实现?" tool_name="list_dir"
    //       tool_result=make_list_dir_result()
    //       mock LLM → UnifiedResponse{text: {"summary":"目录包含 engine.py..."}}
    // refine() → call_llm_structured → from_str → RefinedToolResult{summary}
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    let tool_result = make_list_dir_result();
    let mock_summary = "目录包含 engine.py, config.py, tests/";
    client.set_response(mock_refined_response(mock_summary));

    let result = tr.refine("回测怎么实现?", "list_dir", &tool_result, &client).await;
    assert!(result.is_ok(), "refine must return Ok");
    let refined = result.unwrap();
    assert!(refined.summary.contains("engine.py"),
        "summary must contain actual file name from input directory");
}

#[tokio::test]
async fn tr_014_empty_search_result_refinement() {
    // 输入: question="回测怎么实现?" tool_name="search_content"
    //       tool_result=make_empty_search_result()
    //       mock LLM → UnifiedResponse{text: {"summary":"search_content 返回 0 条匹配..."}}
    // refine() → call_llm_structured → from_str → RefinedToolResult{summary}
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    let tool_result = make_empty_search_result();
    let mock_summary = "search_content 返回 0 条匹配，无相关结果";
    client.set_response(mock_refined_response(mock_summary));

    let result = tr.refine("回测怎么实现?", "search_content", &tool_result, &client).await;
    assert!(result.is_ok(), "refine must return Ok");
    let refined = result.unwrap();
    let s = &refined.summary;
    assert!(s.contains('0') || s.contains("无匹配") || s.contains("无结果"),
        "summary must clearly express zero results");
}

// ============================================================================
// 7.3.2 降级处理 — 仅验证 TR 方法返回值，降级逻辑由 DE 层覆盖
// ============================================================================

#[tokio::test]
async fn tr_020_llm_call_failure_returns_err() {
    // 输入: mock client → Err("LLM call failed")
    // refine() → call_llm_structured → ? 传播 Err
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    client.set_response(Err("LLM call failed".to_string()));

    let result = tr.refine("test", "search_content", &make_search_content_result(), &client).await;
    assert!(result.is_err(), "refine must return Err on LLM failure");
}

#[tokio::test]
async fn tr_021_empty_summary_is_ok_structurally() {
    // 输入: mock LLM → UnifiedResponse{text: {"summary":""}}
    // refine() → call_llm_structured → from_str → RefinedToolResult{summary: ""}
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    client.set_response(mock_refined_response(""));

    let result = tr.refine("test", "search_content", &make_search_content_result(), &client).await;
    assert!(result.is_ok(), "TR refine must return Ok (empty summary is structurally valid)");
    let refined = result.unwrap();
    assert!(refined.summary.is_empty(), "summary must be empty");
}

#[tokio::test]
async fn tr_022_invalid_json_returns_err() {
    // 输入: mock LLM → UnifiedResponse{text: "plain text, not JSON"}
    // refine() → call_llm_structured → from_str 失败 → Err
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    client.set_response(mock_pure_text("plain text, not JSON"));

    let result = tr.refine("test", "search_content", &make_search_content_result(), &client).await;
    assert!(result.is_err(), "refine must return Err on invalid JSON");
}

#[tokio::test]
async fn tr_023_hallucination_passes_through_no_semantic_check() {
    // 输入: mock LLM → valid JSON with invented content unrelated to tool_result
    // refine() → call_llm_structured → from_str → RefinedToolResult{summary}
    // 当前设计不做语义校验，内容直接通过
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    let tool_result = make_search_content_result(); // actual entities: backtest-related
    client.set_response(mock_refined_response(
        "发现用户认证模块在 auth.py 中实现，含 login/logout 方法",
    ));

    let result = tr.refine("回测怎么实现?", "search_content", &tool_result, &client).await;
    assert!(result.is_ok(), "hallucinated content passes through (design choice — no semantic check)");
    let refined = result.unwrap();
    assert!(!refined.summary.contains("src/backtest/engine.py"),
        "hallucinated summary does NOT reference actual tool_result entities");
    assert!(refined.summary.contains("auth.py"),
        "hallucinated summary contains invented entities (known risk, see design doc 6.1)");
}

// ============================================================================
// 7.3.3 输入截断 — 仅验证 TR 对已截断数据的处理，截断逻辑由 DE 层覆盖
// ============================================================================

#[tokio::test]
async fn tr_030_refine_accepts_truncated_data() {
    // 输入: 模拟 DE 层截断后的紧凑数据（≤4000 chars）
    // refine() → call_llm_structured → from_str → Ok(RefinedToolResult)
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    // Simulate what DE layer would pass after truncation: top 10 matches only
    let truncated = serde_json::json!({
        "total_matches": 200,
        "top_matches": (0..10).map(|i| serde_json::json!({
            "file": format!("src/module_{}.py", i),
            "line": (i * 10 + 1).to_string(),
            "content": format!("def func_{}(): pass", i)
        })).collect::<Vec<_>>(),
        "truncated": true
    });

    client.set_response(mock_refined_response("大型搜索结果提炼摘要"));
    let result = tr.refine("test", "search_content", &truncated, &client).await;
    assert!(result.is_ok(), "refine must accept truncated input");
}

#[tokio::test]
async fn tr_031_refine_with_minimal_truncated_data() {
    // 输入: 截断后仅含统计数字 + 少量 top_matches
    // refine() → call_llm_structured → from_str → Ok(RefinedToolResult)
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();
    let minimal = serde_json::json!({
        "total_matches": 1,
        "top_matches": [{"file": "src/main.rs", "line": "1", "content": "fn main()"}],
    });

    client.set_response(mock_refined_response("ok"));
    let result = tr.refine("test", "search_content", &minimal, &client).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tr_032_refine_call_count_per_tool_call() {
    // 输入: 连续 3 次 refine() 调用（模拟 DE 循环中 3 次 tool_call）
    // 每次调用 refine() → call_llm_structured → from_str → Ok(RefinedToolResult)
    let tr = ToolResultRefinerAgent::new();
    let client = MockLlmClient::new();

    for i in 0..3 {
        client.set_response(mock_refined_response(&format!("round {}", i + 1)));
        let result = tr.refine("test", "search_content", &make_search_content_result(), &client).await;
        assert!(result.is_ok(), "refine call {} must succeed", i + 1);
        assert!(!result.unwrap().summary.is_empty(), "summary must be non-empty");
    }
    // done 不触发 TR — TR 本身无状态，此约束由 DE 层落实
}

// ============================================================================
// 附加: RefinedToolResult 数据结构往返
// ============================================================================

#[test]
fn refined_tool_result_roundtrip() {
    let original = RefinedToolResult {
        summary: "匹配到 5 个文件：src/main.rs (line 10: fn main())".to_string(),
    };
    let json = serde_json::to_string(&original).expect("serialize must succeed");
    let deserialized: RefinedToolResult = serde_json::from_str(&json).expect("deserialize must succeed");
    assert_eq!(original.summary, deserialized.summary);
}

#[test]
fn refined_tool_result_empty_summary() {
    let original = RefinedToolResult { summary: String::new() };
    let json = serde_json::to_string(&original).expect("serialize must succeed");
    let deserialized: RefinedToolResult = serde_json::from_str(&json).expect("deserialize must succeed");
    assert_eq!(original.summary, deserialized.summary);
    assert!(deserialized.summary.is_empty());
}
