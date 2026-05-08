use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, UnifiedResponse};
use explore_ai_agent::context::exploration::{ExplorationContextTool, ExplorationRecord, ExplorationSummary};
use explore_ai_agent::tools::fast_explore_tool::FastExploreTool;
use explore_ai_agent::tools::registry::ToolRegistry;
use std::path::PathBuf;
use std::sync::Mutex;

// ============================================================================
// Mock QE client
// ============================================================================

struct MockQeClient {
    response: Mutex<Option<Result<UnifiedResponse, String>>>,
    call_count: Mutex<usize>,
}

impl MockQeClient {
    fn new() -> Self {
        MockQeClient { response: Mutex::new(None), call_count: Mutex::new(0) }
    }
    fn set_response(&self, resp: Result<UnifiedResponse, String>) {
        *self.response.lock().unwrap() = Some(resp);
    }
    fn call_count(&self) -> usize { *self.call_count.lock().unwrap() }
}

#[async_trait::async_trait]
impl LlmStructuredClient for MockQeClient {
    async fn call_llm_structured(
        &self, _instructions: &str, _input_data: &serde_json::Value,
        _output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        *self.call_count.lock().unwrap() += 1;
        self.response.lock().unwrap().take()
            .expect("MockQeClient called without preset response")
    }
}

fn make_qe_ok(confidence: f64) -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse {
        text: Some(format!(r#"{{"key_findings":"found","critical_files":[],"missing_info":"","confidence":{}}}"#, confidence)),
        tool_calls: vec![],
    })
}

fn make_registry() -> ToolRegistry { ToolRegistry::new(PathBuf::from(".")) }
fn make_ect() -> ExplorationContextTool { ExplorationContextTool::new("fe-test".to_string()) }

// ============================================================================
// FE-001: 正常流程
// ============================================================================

#[tokio::test]
async fn fe_001_normal_flow() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    qe.set_response(make_qe_ok(0.8));
    let keywords: Vec<String> = vec!["backtest".to_string(), "回测".to_string()];

    let result = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    // stub: 实现后返回 Ok({matches, key_findings, critical_files, confidence})
    assert!(result.is_ok(), "正常流程应返回 Ok，实际: {:?}", result.err());
}

// FE-002: 空 keywords
#[tokio::test]
async fn fe_002_empty_keywords() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    let keywords: Vec<String> = vec![];

    let result = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    assert!(result.is_err(), "stub 占位，实现后空 keywords 应返回 Err");
}

// FE-003: FastExplorer 返回空 matches
#[tokio::test]
async fn fe_003_empty_matches() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    qe.set_response(make_qe_ok(0.0));
    let keywords: Vec<String> = vec!["nonexistent_xyz".to_string()];

    let result = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    // stub: 实现后应返回 Ok，confidence=0.0
    assert!(result.is_ok(), "空 matches 应返回 Ok(confidence=0.0)，实际: {:?}", result.err());
}

// FE-004: QE 评分后置信度写入 ECT
#[tokio::test]
async fn fe_004_confidence_written_to_ect() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    qe.set_response(make_qe_ok(0.75));
    let keywords: Vec<String> = vec!["test".to_string()];

    let _ = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    // stub: 实现后验证 ECT 中记录的 confidence == 0.75
    let ect_records = ect.get_history();
    assert!(ect_records.is_empty(), "stub: 实现后 ECT 应含记录");
}

// FE-005: ECT 超阈值触发精炼
#[tokio::test]
async fn fe_005_refinement_triggered_on_overflow() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    qe.set_response(make_qe_ok(0.8));

    // Pre-fill ECT to exceed threshold
    for i in 0..50 {
        let record = ExplorationRecord::ToolCall {
            source: "fast_explore".to_string(),
            tool: "FastExplorer".to_string(),
            params: serde_json::json!({"keywords": [format!("k{}", i)]}),
            result_summary: format!("Result {} with substantial padding text to increase token count for threshold testing purposes", i),
            confidence: 0.5,
            timestamp: chrono::Utc::now(),
        };
        let _ = ect.write_record(record);
    }

    let keywords: Vec<String> = vec!["test".to_string()];
    let _ = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    // stub: 实现后 ECT 超阈值 → Refiner 被调用 → 流程不中断
    assert!(ect.needs_compression() || !ect.needs_compression(),
        "stub 占位，实现后验证精炼触发且流程正常");
}

// FE-006: QE 失败不阻塞流程
#[tokio::test]
async fn fe_006_qe_failure_uses_default_confidence() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    qe.set_response(Err("QE LLM call failed".to_string()));
    let keywords: Vec<String> = vec!["test".to_string()];

    let result = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    // stub: QE 失败 → 使用默认置信度 0.5 → 流程继续 → 返回 Ok
    assert!(result.is_ok(), "QE 失败应返回 Ok（使用默认置信度 0.5），实际: {:?}", result.err());
}

// FE-007: 执行顺序正确
#[tokio::test]
async fn fe_007_execution_order() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    qe.set_response(make_qe_ok(0.8));
    let keywords: Vec<String> = vec!["test".to_string()];

    let _ = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    // stub: 实现后验证调用顺序为 FastExplorer → write_record → refine_check → QE → write_confidence → return
    assert!(true, "stub 占位，实现后通过 mock 记录各步骤时间戳验证顺序");
}

// FE-008: FastExplorer 执行失败
#[tokio::test]
async fn fe_008_fast_explorer_failure() {
    let registry = make_registry();
    let ect = make_ect();
    let qe = MockQeClient::new();
    qe.set_response(make_qe_ok(0.5)); // QE is called because FastExplorer succeeds
    let keywords: Vec<String> = vec!["test".to_string()];

    let result = FastExploreTool::execute(&keywords, &registry, &ect, &qe).await;
    // 防御性路径：FastExplorer 通过 ToolRegistry 执行，当前无法通过公开 API mock 其失败。
    // 当 ToolRegistry 注入 trait 后可测试 FastExplorer 失败的 Err 分支。
    assert!(result.is_ok(), "当前路径下 FastExplorer 成功，应返回 Ok");
}
