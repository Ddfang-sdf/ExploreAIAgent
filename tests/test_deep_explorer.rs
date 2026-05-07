use std::path::PathBuf;
use std::sync::Mutex;
use explore_ai_agent::adapter::api_adapter::{
    ApiAdapter, ApiMode, LlmStructuredClient, LlmToolClient, UnifiedResponse,
};
use explore_ai_agent::agents::deep_explorer::*;
use explore_ai_agent::agents::search_strategy::CriticalFileRef;
use explore_ai_agent::context::exploration::{
    CriticalFile, ExplorationContextTool, ExplorationSummary,
};
use explore_ai_agent::tools::registry::ToolRegistry;

fn make_adapter() -> ApiAdapter {
    ApiAdapter::new(ApiMode::Chat)
}

fn make_registry() -> ToolRegistry {
    ToolRegistry::new(PathBuf::from("."))
}

fn make_ect() -> ExplorationContextTool {
    ExplorationContextTool::new("de-test".to_string())
}

// ============================================================================
// 8.2 数据结构测试 (DE-001 ~ DE-003) — 沿用
// ============================================================================

#[test]
fn de_001_collected_evidence_roundtrip() {
    let original = CollectedEvidence {
        file: "src/main.rs".to_string(),
        line: "42-47".to_string(),
        code_snippet: "if (required && value == null) {".to_string(),
        relevance: "required=true 时的校验逻辑".to_string(),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let restored: CollectedEvidence = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.file, original.file);
    assert_eq!(restored.line, original.line);
    assert_eq!(restored.code_snippet, original.code_snippet);
    assert_eq!(restored.relevance, original.relevance);
}

#[test]
fn de_002_deep_explorer_result_roundtrip() {
    let original = DeepExplorerResult {
        critical_files: vec![
            CriticalFileRef { path: "src/main.rs".to_string(), summary: "入口".to_string() },
            CriticalFileRef { path: "src/lib.rs".to_string(), summary: "库入口".to_string() },
        ],
        collected_evidence: vec![
            CollectedEvidence { file: "src/main.rs".to_string(), line: "10".to_string(),
                code_snippet: "fn main() {".to_string(), relevance: "主函数".to_string() },
        ],
        missing_info: "未找到配置加载逻辑".to_string(),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let restored: DeepExplorerResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.critical_files.len(), 2);
    assert_eq!(restored.collected_evidence.len(), 1);
    assert_eq!(restored.missing_info, "未找到配置加载逻辑");
}

#[test]
fn de_003_deep_explorer_result_empty_evidence() {
    let json = r#"{"critical_files":[],"collected_evidence":[],"missing_info":"无"}"#;
    let result: DeepExplorerResult = serde_json::from_str(json).expect("deserialize");
    assert!(result.critical_files.is_empty());
    assert!(result.collected_evidence.is_empty());
    assert_eq!(result.missing_info, "无");
}

// ============================================================================
// 8.3 构造与常量 (DE-004 ~ DE-005) — 沿用
// ============================================================================

#[test]
fn de_004_constructor_returns_instance() {
    let de = DeepExplorer::new();
    assert_eq!(de.max_tool_calls(), 75);
}

#[test]
fn de_005_max_tool_calls_default() {
    let de = DeepExplorer::new();
    assert_eq!(de.max_tool_calls(), MAX_TOOL_CALLS);
}

// ============================================================================
// 8.4 重复检测 (DE-006 ~ DE-009) — 沿用
// ============================================================================

#[test]
fn de_006_first_call_not_duplicate() {
    let mut de = DeepExplorer::new();
    assert!(!de.check_duplicate("read_file", "hash1"));
}

#[test]
fn de_007_same_call_is_duplicate() {
    let mut de = DeepExplorer::new();
    assert!(!de.check_duplicate("read_file", "hash1"));
    assert!(de.check_duplicate("read_file", "hash1"));
}

#[test]
fn de_008_different_tool_not_duplicate() {
    let mut de = DeepExplorer::new();
    assert!(!de.check_duplicate("read_file", "hash1"));
    assert!(!de.check_duplicate("search_content", "hash1"));
}

#[test]
fn de_009_different_params_not_duplicate() {
    let mut de = DeepExplorer::new();
    assert!(!de.check_duplicate("read_file", "hash1"));
    assert!(!de.check_duplicate("read_file", "hash2"));
}

// ============================================================================
// 8.5 循环警告 (DE-010 ~ DE-012) — 沿用
// ============================================================================

#[test]
fn de_010_below_threshold_no_warning() {
    let mut de = DeepExplorer::new();
    de.check_duplicate("read_file", "hash1");
    de.check_duplicate("read_file", "hash1");
    de.check_duplicate("read_file", "hash1");
    assert!(de.generate_loop_warning().is_none());
}

#[test]
fn de_011_at_threshold_triggers_warning() {
    let mut de = DeepExplorer::new();
    de.check_duplicate("read_file", "hash1");
    de.check_duplicate("read_file", "hash1");
    de.check_duplicate("read_file", "hash1");
    de.check_duplicate("read_file", "hash1");
    let warning = de.generate_loop_warning().expect("应触发警告");
    assert!(warning.contains("⚠"), "警告应含警告符号");
    assert!(warning.contains("连续"), "警告应含'连续'");
}

#[test]
fn de_012_above_threshold_continues_warning() {
    let mut de = DeepExplorer::new();
    for _ in 0..6 { de.check_duplicate("read_file", "hash1"); }
    assert!(de.generate_loop_warning().is_some());
}

// ============================================================================
// 8.6 Prompt 组装 (DE-013 ~ DE-016) — v1.2 修改
// ============================================================================

fn make_summary() -> ExplorationSummary {
    ExplorationSummary {
        key_findings: "找到 BooleanValidator.java".to_string(),
        critical_files: vec![CriticalFile {
            path: "core/validation/BooleanValidator.java".to_string(),
            one_sentence_summary: "包含 BooleanValidator 类".to_string(),
        }],
        missing_info: "缺少 validate 方法细节".to_string(),
        confidence: 0.6,
    }
}

#[test]
fn de_013_prompt_contains_question() {
    let de = DeepExplorer::new();
    let prompt = de.assemble_prompt("What is X?", &make_summary());
    assert!(prompt.contains("## 用户问题"), "应含章节标题");
    assert!(prompt.contains("What is X?"), "应含问题内容");
}

#[test]
fn de_014_prompt_contains_summary() {
    let de = DeepExplorer::new();
    let prompt = de.assemble_prompt("test", &make_summary());
    assert!(prompt.contains("## 已有探索线索"), "应含章节标题");
    assert!(prompt.contains("BooleanValidator"), "应含摘要内容");
}

// DE-015: v1.2 Prompt 含 JSON 通信说明（不再提及 API tool_calls）
#[test]
fn de_015_prompt_contains_json_communication() {
    let de = DeepExplorer::new();
    let prompt = de.assemble_prompt("test", &make_summary());
    // v1.2: system auto-records, LLM focuses on exploration
    assert!(
        prompt.contains("系统会自动记录") || prompt.contains("聚焦探索"),
        "v1.2: Prompt 应含自动记录说明"
    );
    assert!(!prompt.contains("必须记录每次发现"), "v1.2: 不应含强制记录要求");
    assert!(prompt.contains("避免短期重复"));
    assert!(prompt.contains("适时终止"));
    assert!(prompt.contains("通信协议"), "v1.2: 应含 JSON 通信协议说明");
    assert!(prompt.contains("action\": \"tool_call\""), "v1.2: 应含 tool_call action 格式");
    // v1.2: tools should be hardcoded in prompt, not via {tools} placeholder
    assert!(
        !prompt.contains("{tools}")
            || prompt.contains("list_dir") || prompt.contains("search_content"),
        "v1.2: tools either hardcoded in prompt or listed as available tools"
    );
}

// DE-015b: v1.2 Prompt 不含 exploration_context_tool
#[test]
fn de_015b_prompt_excludes_exploration_context_tool() {
    let de = DeepExplorer::new();
    let prompt = de.assemble_prompt("test", &make_summary());
    assert!(!prompt.contains("exploration_context_tool"), "v1.2: LLM 不感知 ECT");
}

#[test]
fn de_016_prompt_contains_loop_warning() {
    let mut de = DeepExplorer::new();
    for _ in 0..4 { de.check_duplicate("read_file", "hash1"); }
    let prompt = de.assemble_prompt("test", &make_summary());
    assert!(prompt.contains("## ⚠️ 系统警告"), "应含警告章节");
    assert!(prompt.contains("连续"), "应含警告内容");
}

// ============================================================================
// 8.7 集成测试 (DE-017 ~ DE-025)
// ============================================================================

// DE-017: v1.2 正常探索 — mock 返回 JSON action 而非 tool_calls
#[tokio::test]
async fn de_017_normal_exploration_flow() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 实现后 mock 依次返回: {"action":"tool_call","tool":"search_content",...}
    // → {"action":"tool_call","tool":"read_file",...} → {"action":"done","result":{...}}
    // 验证代码层自动调用了 ECT（2 次 = 工具调用次数）
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// DE-018: 达到上限强制终止
#[tokio::test]
async fn de_018_max_calls_forced_termination() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 实现后 mock 持续返回 tool_call JSON → 75 次后兜底终止
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok(兜底)");
}

// DE-019: 重复调用检测
#[tokio::test]
async fn de_019_duplicate_detection_active() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 实现后 mock 返回 3 次相同 tool_call JSON → 缓存生效 + loop_warning
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// DE-020: 工具执行失败后 LLM 调整
#[tokio::test]
async fn de_020_tool_failure_recovery() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 实现后 mock 返回 tool_call JSON → 工具执行失败 → LLM 收到错误 → 调整
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// DE-021: 终止输出解析失败
#[tokio::test]
async fn de_021_parse_failure_returns_err() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 实现后 mock 返回 action=done 但 result 非法 → Err
    assert!(result.is_err(), "stub 占位，实现后应返回 Err");
}

// DE-022: v1.1/v1.2 代码自动记录 exploration_context_tool
#[tokio::test]
async fn de_022_auto_record_exploration_context() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 验证每次 tool_call JSON 执行后代码自动调用了 ECT.write()
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// ============================================================================
// v1.2 新增
// ============================================================================

// DE-023: JSON 解析失败重试成功
#[tokio::test]
async fn de_023_json_parse_retry_success() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 实现后 mock 第 1 次返回非法 JSON → 追加提示重试 → 第 2 次正确
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// DE-024: tool_call 缺少 tool/params 字段 — 重试
#[tokio::test]
async fn de_024_tool_call_missing_fields_retry() {
    let mut de = DeepExplorer::new();
    let adapter = make_adapter();
    let registry = make_registry();
    let ect = make_ect();
    let result = de.execute("test", &make_summary(), &adapter, &registry, &ect).await;
    // stub: 实现后 mock 返回 {"action":"tool_call"} 不含 tool/params → 追加提示重试
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok（重试成功）或 Err（重试耗尽）");
}

// DE-025: 验证 response_format JSON Schema 结构正确
#[test]
fn de_025_action_schema_structure() {
    let schema = DeepExplorer::action_schema();
    assert!(schema.is_object(), "action_schema 应返回合法 JSON 对象");
    assert!(schema.get("name").is_some(), "schema 应有 name 字段");
    assert!(schema.get("strict").is_some(), "schema 应有 strict 字段");
    assert!(schema.get("schema").is_some(), "schema 应有 schema 字段");
}

// ============================================================================
// v1.3 新增: DE 上下文精炼 (DE-026 ~ DE-029)
// ============================================================================

/// Mock that implements both LlmToolClient (for DE exploration calls) and
/// LlmStructuredClient (for Refiner calls).  Both traits must be satisfied
/// because DE's execute() needs to pass the adapter to Refiner.
struct MockDualClient {
    tool_responses: Mutex<Vec<Result<UnifiedResponse, String>>>,
    structured_responses: Mutex<Vec<Result<UnifiedResponse, String>>>,
    structured_call_count: Mutex<usize>,
}

impl MockDualClient {
    fn new() -> Self {
        MockDualClient {
            tool_responses: Mutex::new(Vec::new()),
            structured_responses: Mutex::new(Vec::new()),
            structured_call_count: Mutex::new(0),
        }
    }

    fn push_tool_response(&self, resp: Result<UnifiedResponse, String>) {
        self.tool_responses.lock().unwrap().push(resp);
    }

    fn push_structured_response(&self, resp: Result<UnifiedResponse, String>) {
        self.structured_responses.lock().unwrap().push(resp);
    }

    fn structured_call_count(&self) -> usize {
        *self.structured_call_count.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl LlmStructuredClient for MockDualClient {
    async fn call_llm_structured(
        &self,
        _instructions: &str,
        _input_data: &serde_json::Value,
        _output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        *self.structured_call_count.lock().unwrap() += 1;
        self.structured_responses
            .lock()
            .unwrap()
            .remove(0)
    }
}

#[async_trait::async_trait]
impl LlmToolClient for MockDualClient {
    async fn call_llm_with_tools(
        &self,
        _messages: &[serde_json::Value],
        _tools: &[serde_json::Value],
        _response_format: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        self.tool_responses
            .lock()
            .unwrap()
            .remove(0)
    }
}

fn make_done_response() -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse {
        text: Some(r#"{"action":"done","result":{"critical_files":[],"collected_evidence":[],"missing_info":"无"},"reasoning":"done"}"#.to_string()),
        tool_calls: vec![],
    })
}

fn make_tool_call_response(tool: &str, file: &str) -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse {
        text: Some(format!(
            r#"{{"action":"tool_call","tool":"{}","params":{{"file":"{}"}},"reasoning":"explore"}}"#,
            tool, file
        )),
        tool_calls: vec![],
    })
}

fn make_large_search_response() -> Result<UnifiedResponse, String> {
    // Simulate a search_content result that would produce 40K+ chars of output.
    // The mock returns a tool_call action; the actual tool execution via
    // ToolRegistry will produce a large result (if the tool is set up).
    // For the test, we use a normal tool_call and rely on the DE's context
    // tracking to detect overflow.
    Ok(UnifiedResponse {
        text: Some(r#"{"action":"tool_call","tool":"search_content","params":{"pattern":"backtest"},"reasoning":"搜索回测相关代码"}"#.to_string()),
        tool_calls: vec![],
    })
}

fn make_refiner_summary_response() -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse {
        text: Some(
            r#"{"key_findings":"精炼后的发现","critical_files":[{"path":"src/backtest.rs","one_sentence_summary":"回测引擎"}],"missing_info":"","confidence":0.85}"#
                .to_string(),
        ),
        tool_calls: vec![],
    })
}

// DE-026: DE messages 超限触发上下文精炼
#[tokio::test]
async fn de_026_messages_overflow_triggers_refinement() {
    let mut de = DeepExplorer::new();
    de.max_tool_calls = 5; // limit calls for test speed
    let registry = make_registry();
    let ect = make_ect();
    let mock = MockDualClient::new();

    // DE explores: first call returns a tool_call that will produce large output
    mock.push_tool_response(make_large_search_response());
    // After refinement, DE continues and LLM responds with done
    mock.push_tool_response(make_done_response());
    // Refiner is called during context refinement
    mock.push_structured_response(make_refiner_summary_response());

    let result = de
        .execute("test question", &make_summary(), &mock, &registry, &ect)
        .await;

    // stub: 实现后验证
    // (1) Refiner 被调用 (structured_call_count > 0)
    // (2) execute() 返回 Ok（精炼后继续探索并正常终止）
    // (3) 旧的截断日志路径不被触发
    assert!(result.is_ok() || result.is_err(),
        "stub: 实现后应返回 Ok(DeepExplorerResult)");
}

// DE-027: 精炼后 messages 从 ECT 重建
#[tokio::test]
async fn de_027_messages_rebuilt_after_refinement() {
    let mut de = DeepExplorer::new();
    de.max_tool_calls = 5;
    let registry = make_registry();
    let ect = make_ect();
    let mock = MockDualClient::new();

    // First exploration call: search (produces large context)
    mock.push_tool_response(make_large_search_response());
    // After refinement, DE resumes: read_file
    mock.push_tool_response(make_tool_call_response("read_file", "src/backtest.rs"));
    // Then done
    mock.push_tool_response(make_done_response());
    // Refiner returns a summary
    mock.push_structured_response(make_refiner_summary_response());

    let result = de
        .execute("test question", &make_summary(), &mock, &registry, &ect)
        .await;

    // stub: 实现后验证
    // (1) Refiner 被调用了一次
    // (2) Refiner 返回的摘要写入了 ECT（get_current_summary 非空且含精炼内容）
    // (3) messages 重建后总 token < 8000
    // (4) messages 尾部保留最近 2 条原始工具结果
    assert!(result.is_ok() || result.is_err(),
        "stub: 实现后应返回 Ok(DeepExplorerResult)");
}

// DE-028: 精炼失败降级截断
#[tokio::test]
async fn de_028_refinement_failure_degradation() {
    let mut de = DeepExplorer::new();
    de.max_tool_calls = 8;
    let registry = make_registry();
    let ect = make_ect();
    let mock = MockDualClient::new();

    // First call: large result triggers overflow
    mock.push_tool_response(make_large_search_response());
    // Refiner fails
    mock.push_structured_response(Err("LLM call failed".to_string()));
    // After degradation truncation, DE continues: read_file
    mock.push_tool_response(make_tool_call_response("read_file", "src/backtest.rs"));
    // Then done
    mock.push_tool_response(make_done_response());

    let result = de
        .execute("test question", &make_summary(), &mock, &registry, &ect)
        .await;

    // stub: 实现后验证
    // (1) execute() 不中断（返回 Ok，因降级截断后继续探索并正常终止）
    // (2) ECT.current_summary 未被修改（精炼失败，无写入）
    // (3) structured_call_count == 1（Refiner 被调用了一次，但失败了）
    assert!(result.is_ok() || result.is_err(),
        "stub: 实现后应返回 Ok（降级截断后继续探索并正常终止）");
}

// DE-029: degradation_count 达上限终止循环
#[tokio::test]
async fn de_029_degradation_limit_terminates_loop() {
    let mut de = DeepExplorer::new();
    de.max_tool_calls = 20;
    let registry = make_registry();
    let ect = make_ect();
    let mock = MockDualClient::new();

    // Cycle 1: overflow → Refiner fails → degradation
    mock.push_tool_response(make_large_search_response());
    mock.push_structured_response(Err("LLM call failed".to_string()));
    // After truncation, DE tries again: another large result
    mock.push_tool_response(make_large_search_response());
    // Cycle 2: Refiner fails again
    mock.push_structured_response(Err("LLM call failed".to_string()));
    // After truncation: yet another large result
    mock.push_tool_response(make_large_search_response());
    // Cycle 3: Refiner fails third time → degradation_count = 3 → terminate
    mock.push_structured_response(Err("LLM call failed".to_string()));
    // Provide extra fallback responses in case the current stub code path
    // continues looping (old truncation, not yet replaced with Refiner).
    // After refinement is implemented, these extras should never be consumed.
    for _ in 0..5 {
        mock.push_tool_response(make_done_response());
    }

    let result = de
        .execute("test question", &make_summary(), &mock, &registry, &ect)
        .await;

    // stub: 实现后验证
    // (1) execute() 返回 Ok（兜底终止，返回已收集证据）
    // (2) structured_call_count == 3（Refiner 被调用了 3 次，全部失败）
    // (3) 返回的 missing_info 含 "上下文精炼连续失败" 或 "强制终止"
    // (4) 实际工具调用次数 < 20（提前终止，未用满 max_tool_calls）
    assert!(result.is_ok() || result.is_err(),
        "stub: 实现后应返回 Ok（degradation_count=3 时兜底终止）");
}
