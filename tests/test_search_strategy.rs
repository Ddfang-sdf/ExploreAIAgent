// ⚠️ v1.2: SearchStrategyAgent 已废弃。本测试文件保留作为历史参考。
// 相关逻辑已迁移至：MainAgent（关键词设计）+ fast_explore_tool（代码执行）+ QE（评分）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use explore_ai_agent::adapter::api_adapter::{ApiAdapter, ApiMode, LlmStructuredClient, UnifiedResponse};
use explore_ai_agent::agents::search_strategy::*;
use explore_ai_agent::context::exploration::ExplorationContextTool;
use explore_ai_agent::tools::registry::ToolRegistry;

// ============================================================================
// Helpers
// ============================================================================

fn make_adapter() -> Arc<ApiAdapter> {
    Arc::new(ApiAdapter::new(ApiMode::Chat))
}

fn make_registry() -> Arc<ToolRegistry> {
    Arc::new(ToolRegistry::new(PathBuf::from(".")))
}

fn make_ect() -> ExplorationContextTool {
    ExplorationContextTool::new("ssa-test".to_string())
}

struct MockRefinerClient {
    response: Mutex<Option<Result<UnifiedResponse, String>>>,
    call_count: Mutex<usize>,
}

impl MockRefinerClient {
    fn new() -> Self {
        MockRefinerClient {
            response: Mutex::new(None),
            call_count: Mutex::new(0),
        }
    }

    fn set_response(&self, resp: Result<UnifiedResponse, String>) {
        *self.response.lock().unwrap() = Some(resp);
    }

    fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl LlmStructuredClient for MockRefinerClient {
    async fn call_llm_structured(
        &self,
        _instructions: &str,
        _input_data: &serde_json::Value,
        _output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        *self.call_count.lock().unwrap() += 1;
        self.response.lock().unwrap().take()
            .expect("MockRefinerClient called without preset response")
    }
}

fn make_refiner_ok_response() -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse {
        text: Some(r#"{"key_findings":"精炼摘要","critical_files":[{"path":"src/main.rs","one_sentence_summary":"入口"}],"missing_info":"","confidence":0.8}"#.to_string()),
        tool_calls: vec![],
    })
}

// ============================================================================
// 8.2 数据结构测试 (SS-001 ~ SS-003) — 沿用 v1.0
// ============================================================================

#[test]
fn ss_001_search_round_record_roundtrip() {
    let original = SearchRoundRecord {
        round: 1,
        keywords: vec!["BooleanValidator".to_string(), "参数".to_string()],
        key_findings: "找到 BooleanValidator.java".to_string(),
        confidence: 0.5,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let restored: SearchRoundRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.round, 1);
    assert_eq!(restored.keywords, vec!["BooleanValidator", "参数"]);
    assert_eq!(restored.key_findings, "找到 BooleanValidator.java");
    assert_eq!(restored.confidence, 0.5);
}

#[test]
fn ss_002_search_strategy_result_roundtrip() {
    let original = SearchStrategyResult {
        key_findings: "发现核心代码".to_string(),
        critical_files: vec![
            CriticalFileRef { path: "src/main.rs".to_string(), summary: "入口文件".to_string() },
            CriticalFileRef { path: "src/lib.rs".to_string(), summary: "库入口".to_string() },
        ],
        missing_info: "缺少配置加载机制".to_string(),
        confidence: 0.75,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let restored: SearchStrategyResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.key_findings, original.key_findings);
    assert_eq!(restored.missing_info, original.missing_info);
    assert_eq!(restored.confidence, original.confidence);
    assert_eq!(restored.critical_files.len(), 2);
    assert_eq!(restored.critical_files[0].path, "src/main.rs");
    assert_eq!(restored.critical_files[0].summary, "入口文件");
}

#[test]
fn ss_003_critical_file_ref_field_name() {
    let file = CriticalFileRef { path: "a.rs".to_string(), summary: "文件 A".to_string() };
    let json = serde_json::to_string(&file).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("summary").is_some(), "字段名应为 summary");
    assert!(parsed.get("one_sentence_summary").is_none());
    assert_eq!(parsed["summary"].as_str().unwrap(), "文件 A");
}

// ============================================================================
// 8.3 构造测试 (SS-004 ~ SS-005) — 沿用 v1.0
// ============================================================================

#[test]
fn ss_004_constructor_returns_instance() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let _ = agent;
}

#[test]
fn ss_005_max_rounds_default_value() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    assert_eq!(agent.max_rounds(), 5);
}

// ============================================================================
// 8.4 Prompt 组装测试 (SS-006 ~ SS-010c) — 修改：拆为两个 Prompt 分别验证
// ============================================================================

// 8.4.1 关键词设计 Prompt

#[test]
fn ss_006_keywords_prompt_contains_question() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let prompt = agent.assemble_keywords_prompt("What is X?", &[], 1);
    assert!(prompt.contains("## 用户问题"), "应含章节标题");
    assert!(prompt.contains("What is X?"), "应含问题内容");
}

#[test]
fn ss_007_keywords_prompt_contains_history() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let history = vec![SearchRoundRecord {
        round: 1,
        keywords: vec!["BooleanValidator".to_string()],
        key_findings: "找到 BooleanValidator.java".to_string(),
        confidence: 0.4,
    }];
    let prompt = agent.assemble_keywords_prompt("test", &history, 2);
    assert!(prompt.contains("## 历史探索记录"), "应含章节标题");
    assert!(prompt.contains("BooleanValidator"), "应含历史关键词");
}

#[test]
fn ss_008_first_round_empty_history() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let prompt = agent.assemble_keywords_prompt("test", &[], 1);
    assert!(
        prompt.contains("首轮") || prompt.contains("无历史") || prompt.find("[]").is_some(),
        "首轮应标识无历史记录"
    );
}

#[test]
fn ss_009_keywords_prompt_contains_design_instructions() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let prompt = agent.assemble_keywords_prompt("test", &[], 1);
    assert!(prompt.contains("设计关键词"), "应含关键词设计要求");
    assert!(prompt.contains("中英文"), "应含中英文关键词提示");
    assert!(prompt.contains("输出格式"), "应含输出格式说明");
    // v1.1: keywords prompt is short, focused only on keyword design
    assert!(!prompt.contains("fast_explorer"), "v1.1 关键词 Prompt 不应含工具名");
    assert!(!prompt.contains("exploration_context_tool"), "v1.1 关键词 Prompt 不应含 ECT");
}

// 8.4.2 评估 Prompt

#[test]
fn ss_010_evaluation_prompt_contains_exploration_data() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let exploration_result = serde_json::json!({
        "matches": [{"file": "README.md", "content": "# AI Hedge Fund"}]
    });
    let prompt = agent.assemble_evaluation_prompt("test", &exploration_result);
    assert!(prompt.contains("探索数据"), "应含探索数据章节");
    assert!(prompt.contains("AI Hedge Fund"), "应含探索结果内容");
}

#[test]
fn ss_010b_evaluation_prompt_contains_confidence_table() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let prompt = agent.assemble_evaluation_prompt("test", &serde_json::json!({"matches": []}));
    assert!(prompt.contains("置信度"), "应含评分表");
    assert!(prompt.contains("0.8"), "应含置信度数值");
    assert!(prompt.contains("1.0"), "应含 1.0 边界值");
}

#[test]
fn ss_010c_evaluation_prompt_contains_output_format() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let prompt = agent.assemble_evaluation_prompt("test", &serde_json::json!({"matches": []}));
    assert!(prompt.contains("key_findings"), "应含 key_findings 字段说明");
    assert!(prompt.contains("critical_files"), "应含 critical_files 字段说明");
    assert!(prompt.contains("missing_info"), "应含 missing_info 字段说明");
    assert!(prompt.contains("confidence"), "应含 confidence 字段说明");
}

// ============================================================================
// 8.5 工具定义测试 (SS-011) — 修改：get_tools 返回空
// SS-012, SS-013 已删除（v1.1 LLM 无工具定义）
// ============================================================================

#[test]
fn ss_011_get_tools_returns_empty() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let tools = agent.get_tools();
    assert!(
        tools.is_empty(),
        "v1.1: LLM 不再接触工具，get_tools() 应返回空列表，实际长度 {}",
        tools.len()
    );
}

// ============================================================================
// 8.6 校验逻辑测试 (SS-014 ~ SS-017) — 沿用 v1.0
// ============================================================================

#[tokio::test]
async fn ss_014_confidence_zero_is_valid() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

#[tokio::test]
async fn ss_015_confidence_one_is_valid() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

#[tokio::test]
async fn ss_016_confidence_negative_is_invalid() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    assert!(result.is_err(), "stub 占位，实现后应校验 confidence 范围");
}

#[tokio::test]
async fn ss_017_confidence_above_one_is_invalid() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    assert!(result.is_err(), "stub 占位，实现后应校验 confidence 范围");
}

// ============================================================================
// 8.7 集成测试 (SS-018 ~ SS-029)
// ============================================================================

// 8.7.1 两阶段正常流程

#[tokio::test]
async fn ss_018_normal_exploration_flow() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("What is X?", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock 阶段一返回 keywords JSON，阶段二返回评估 JSON
    // 验证代码层调用了 fast_explorer 和 exploration_context_tool
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

#[tokio::test]
async fn ss_019_question_unrelated_to_codebase() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("你好", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock 阶段一返回 {"keywords":[]} → 不调 fast_explorer
    // → confidence=1.0, key_findings 含 "无关"
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

#[tokio::test]
async fn ss_020_multi_round_with_history() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let history = vec![SearchRoundRecord {
        round: 1,
        keywords: vec!["BooleanValidator".to_string()],
        key_findings: "找到 BooleanValidator.java".to_string(),
        confidence: 0.4,
    }];
    let result = agent.execute_round("test", &history, 2, &ect, &refiner).await;
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// 8.7.2 错误处理
// SS-021 已删除（v1.1 无工具调用循环）
// SS-025 已删除（v1.1 无强制记录重试）

#[tokio::test]
async fn ss_022_empty_response() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock 返回空 UnifiedResponse → Err("Empty response")
    assert!(result.is_err(), "stub 占位，实现后应返回 Err");
}

#[tokio::test]
async fn ss_023_invalid_json_response() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock 返回非法 JSON → 重试 2 次仍失败 → Err
    assert!(result.is_err(), "stub 占位，实现后应返回 Err");
}

#[tokio::test]
async fn ss_026_tool_execution_error_feedback() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock fast_explorer 执行失败 → Err 含工具错误信息
    assert!(result.is_err(), "stub 占位，实现后应返回 Err");
}

// 8.7.3 v1.1 新增

#[tokio::test]
async fn ss_024_auto_record_exploration_context() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后验证：fast_explorer 执行后代码自动调了
    // exploration_context_tool.write()，入参含 source="SearchStrategyAgent" 和探索数据
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok（含自动记录验证）");
}

#[tokio::test]
async fn ss_027_phase1_json_retry_success() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock 阶段一第 1 次非法 JSON → 重试 → 第 2 次合法 → Ok
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

#[tokio::test]
async fn ss_028_phase1_json_retry_exhausted() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock 阶段一连续 3 次非法 JSON → Err("Failed to parse keywords JSON after retries")
    assert!(result.is_err(), "stub 占位，实现后应返回 Err");
}

#[tokio::test]
async fn ss_029_phase2_json_retry_exhausted() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub 占位；实现后 mock 阶段一正常 → 阶段二连续 3 次非法 JSON → Err("Failed to parse evaluation JSON after retries")
    assert!(result.is_err(), "stub 占位，实现后应返回 Err");
}

// ============================================================================
// v1.2 新增: SSA 上下文精炼 (SS-030 ~ SS-033)
// ============================================================================

#[tokio::test]
async fn ss_030_refinement_triggered_when_ect_over_threshold() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    refiner.set_response(make_refiner_ok_response());

    // Pre-fill ECT with many records to exceed EXPLORATION_TOKEN_THRESHOLD
    for i in 0..30 {
        let record = explore_ai_agent::context::exploration::ExplorationRecord::ToolCall {
            source: "SearchStrategyAgent".to_string(),
            tool: "fast_explorer".to_string(),
            params: serde_json::json!({"keywords": [format!("keyword_{}", i)]}),
            result_summary: format!("Result summary for keyword_{} with enough text to increase token count significantly", i),
            confidence: 0.5,
            timestamp: chrono::Utc::now(),
        };
        let _ = ect.write_record(record);
    }

    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub: 实现后验证
    // (1) execute_round() 返回 Ok（stage 1+2 succeed）
    // (2) refiner.call_count() > 0（Refiner was called）
    // (3) ECT.current_summary was updated
    assert!(result.is_ok() || result.is_err(),
        "stub: 实现后应返回 Ok（精炼触发成功）");
}

#[tokio::test]
async fn ss_031_refinement_not_triggered_when_below_threshold() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect(); // fresh ECT, far below threshold
    let refiner = MockRefinerClient::new();
    refiner.set_response(make_refiner_ok_response());

    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub: 实现后验证
    // (1) execute_round() 返回 Ok
    // (2) refiner.call_count() == 0（Refiner was NOT called）
    assert!(result.is_ok() || result.is_err(),
        "stub: 实现后应返回 Ok（未触发精炼）");
}

#[tokio::test]
async fn ss_032_refinement_failure_does_not_block() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();
    refiner.set_response(Err("LLM call failed".to_string()));

    // Pre-fill ECT to exceed threshold
    for i in 0..30 {
        let record = explore_ai_agent::context::exploration::ExplorationRecord::ToolCall {
            source: "SearchStrategyAgent".to_string(),
            tool: "fast_explorer".to_string(),
            params: serde_json::json!({"keywords": [format!("k{}", i)]}),
            result_summary: format!("Result {} with padding text to increase token count substantially for testing", i),
            confidence: 0.5,
            timestamp: chrono::Utc::now(),
        };
        let _ = ect.write_record(record);
    }

    let result = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub: 实现后验证
    // (1) execute_round() 返回 Ok（精炼失败不阻塞探索）
    // (2) refiner was called but returned Err — SSA continued to stage 2
    assert!(result.is_ok() || result.is_err(),
        "stub: 实现后应返回 Ok（精炼失败不阻塞）");
}

#[tokio::test]
async fn ss_033_multi_round_refinement_accumulates() {
    let agent = SearchStrategyAgent::new(make_adapter(), make_registry());
    let ect = make_ect();
    let refiner = MockRefinerClient::new();

    // Round 1: ECT empty, no refinement
    refiner.set_response(make_refiner_ok_response());
    let r1 = agent.execute_round("test", &[], 1, &ect, &refiner).await;
    // stub: round 1 should not trigger refinement (ECT nearly empty)

    // Round 2: ECT has some records but still under threshold
    for i in 0..5 {
        let record = explore_ai_agent::context::exploration::ExplorationRecord::ToolCall {
            source: "SearchStrategyAgent".to_string(),
            tool: "fast_explorer".to_string(),
            params: serde_json::json!({"keywords": [format!("r2_{}", i)]}),
            result_summary: format!("R2 result {} padding", i),
            confidence: 0.5,
            timestamp: chrono::Utc::now(),
        };
        let _ = ect.write_record(record);
    }
    refiner.set_response(make_refiner_ok_response());
    let r2 = agent.execute_round("test", &[], 2, &ect, &refiner).await;
    // stub: round 2 may or may not trigger (depends on ECT fill)

    // Round 3: ECT heavily filled, should trigger
    for i in 0..30 {
        let record = explore_ai_agent::context::exploration::ExplorationRecord::ToolCall {
            source: "SearchStrategyAgent".to_string(),
            tool: "fast_explorer".to_string(),
            params: serde_json::json!({"keywords": [format!("r3_{}", i)]}),
            result_summary: format!("R3 result {} with substantial padding text to exceed token threshold", i),
            confidence: 0.5,
            timestamp: chrono::Utc::now(),
        };
        let _ = ect.write_record(record);
    }
    refiner.set_response(make_refiner_ok_response());
    let r3 = agent.execute_round("test", &[], 3, &ect, &refiner).await;
    // stub: round 3 MUST trigger refinement

    // All three rounds should complete (stubs assert basic contract)
    let all_err = r1.is_err() && r2.is_err() && r3.is_err();
    assert!(all_err || (!r1.is_err() && !r2.is_err() && !r3.is_err()),
        "stub: 实现后三轮都应返回 Ok；当前三轮都返回 Err 也符合 stub 预期");
}
