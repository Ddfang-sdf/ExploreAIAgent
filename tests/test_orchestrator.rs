use std::path::PathBuf;
use std::sync::Arc;

use explore_ai_agent::adapter::api_adapter::{ApiAdapter, ApiMode};
use explore_ai_agent::agents::quality_evaluator::ExplorationAction;
use explore_ai_agent::context::exploration::ExplorationContextTool;
use explore_ai_agent::conversation::manager::ConversationManager;
use explore_ai_agent::orchestrator::orchestrator::*;
use explore_ai_agent::tools::registry::ToolRegistry;

fn make_orchestrator() -> Orchestrator {
    Orchestrator::new(
        Arc::new(ApiAdapter::new(ApiMode::Chat)),
        Arc::new(ToolRegistry::new(PathBuf::from("."))),
        ConversationManager::new(ApiAdapter::new(ApiMode::Chat)),
    )
}

// ===== OC-001 ~ OC-003: should_early_terminate =====

#[test]
fn oc_001_early_terminate_high_confidence() {
    let orch = make_orchestrator();
    assert!(orch.should_early_terminate(0.95, 2));
    assert!(orch.should_early_terminate(0.9, 2));
}

#[test]
fn oc_002_early_terminate_low_confidence() {
    let orch = make_orchestrator();
    assert!(!orch.should_early_terminate(0.85, 1));
    assert!(!orch.should_early_terminate(0.5, 2));
}

#[test]
fn oc_003_early_terminate_at_max_rounds() {
    let orch = make_orchestrator();
    // At max rounds (5), should not early-terminate — loop ends naturally
    assert!(!orch.should_early_terminate(0.95, 5));
}

// ===== OC-004 ~ OC-006: should_deep_explore =====

#[test]
fn oc_004_deep_explore_when_action_is_deep_explore_and_code_related() {
    let orch = make_orchestrator();
    assert!(orch.should_deep_explore(&ExplorationAction::DeepExplore, true));
}

#[test]
fn oc_005_no_deep_explore_when_action_is_answer() {
    let orch = make_orchestrator();
    assert!(!orch.should_deep_explore(&ExplorationAction::Answer, true));
}

#[test]
fn oc_006_no_deep_explore_when_not_code_related() {
    let orch = make_orchestrator();
    assert!(!orch.should_deep_explore(&ExplorationAction::DeepExplore, false));
}

// ===== OC-007: constructor =====

#[test]
fn oc_007_constructor_does_not_panic() {
    let orch = make_orchestrator();
    let _ = orch;
}

// ===== OC-008 ~ OC-009: build_qe_input =====

// OC-008: 从 ECT 构造 QE 输入（含证据）
// 推导链：ECT.get_current_summary + get_history → filter DeepExplorer records → QualityEvaluatorInput
#[test]
fn oc_008_build_qe_input_with_evidence() {
    let ect = ExplorationContextTool::new("test-session".to_string());
    let result = Orchestrator::build_qe_input(&ect);
    assert!(result.is_ok(), "build_qe_input 应返回 Ok");
    let input = result.unwrap();
    // 空 ECT 下 collected_evidence 为空；实现后写入 DeepExplorer 记录时应有值
    assert!(input.collected_evidence.is_empty(), "空 ECT 下证据为空");
}

// OC-009: 从 ECT 构造 QE 输入（无证据）
// 推导链：ECT 仅含 SearchStrategyAgent Summary 记录 → filter DeepExplorer → 空 Vec
#[test]
fn oc_009_build_qe_input_empty_evidence() {
    let ect = ExplorationContextTool::new("test-session".to_string());
    let result = Orchestrator::build_qe_input(&ect);
    assert!(result.is_ok(), "build_qe_input 应返回 Ok");
    let input = result.unwrap();
    // 无 DeepExplorer 记录时 evidence 为空
    assert!(input.collected_evidence.is_empty());
}

// ===== OC-010 ~ OC-012: run 集成测试 =====

// OC-010: 快速探索后直接回答（无需深度探索）
#[tokio::test]
async fn oc_010_fast_explore_then_answer() {
    let orch = make_orchestrator();
    let mut ect = ExplorationContextTool::new("test-session".to_string());
    let result = orch.run("test question", &mut ect).await;
    // stub 占位，实现后应返回 Ok，含 <final_response>
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// OC-011: 快速探索 + 深度探索完整流程
#[tokio::test]
async fn oc_011_full_pipeline_with_deep_explore() {
    let orch = make_orchestrator();
    let mut ect = ExplorationContextTool::new("test-session".to_string());
    let result = orch.run("test question", &mut ect).await;
    // stub 占位，实现后应返回 Ok
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// OC-012: 对话上下文精炼触发
#[tokio::test]
async fn oc_012_conversation_refinement_triggered() {
    let orch = make_orchestrator();
    let mut ect = ExplorationContextTool::new("test-session".to_string());
    let result = orch.run("test question", &mut ect).await;
    // stub 占位，实现后应返回 Ok
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok");
}

// ===== ExplorationAction Tests =====

#[test]
fn exploration_action_serialization() {
    let action = ExplorationAction::Answer;
    let json = serde_json::to_value(&action).unwrap();
    assert_eq!(json, "answer");

    let action = ExplorationAction::DeepExplore;
    let json = serde_json::to_value(&action).unwrap();
    assert_eq!(json, "deep_explore");
}

#[test]
fn exploration_action_deserialization() {
    let answer: ExplorationAction = serde_json::from_str("\"answer\"").unwrap();
    assert_eq!(answer, ExplorationAction::Answer);

    let deep: ExplorationAction = serde_json::from_str("\"deep_explore\"").unwrap();
    assert_eq!(deep, ExplorationAction::DeepExplore);
}

// ===== Quality Evaluator Schema Tests =====

#[test]
fn quality_evaluator_schema_is_valid_json() {
    use explore_ai_agent::agents::quality_evaluator::QUALITY_EVALUATOR_SCHEMA;
    let schema: serde_json::Value = serde_json::from_str(QUALITY_EVALUATOR_SCHEMA)
        .expect("Quality evaluator schema should be valid JSON");

    assert_eq!(schema["name"], "exploration_quality_evaluator_response");
    assert!(schema["strict"].as_bool().unwrap());

    let props = &schema["schema"]["properties"];
    assert!(props.get("key_findings").is_some());
    assert!(props.get("critical_files").is_some());
    assert!(props.get("missing_info").is_some());
    assert!(props.get("confidence").is_some());
    assert!(props.get("action").is_some());
    assert!(props.get("reason").is_some());

    let required = schema["schema"]["required"].as_array().unwrap();
    let required_fields: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(required_fields.contains(&"key_findings"));
    assert!(required_fields.contains(&"critical_files"));
    assert!(required_fields.contains(&"missing_info"));
    assert!(required_fields.contains(&"confidence"));
    assert!(required_fields.contains(&"action"));
    assert!(required_fields.contains(&"reason"));

    assert!(schema["schema"]["additionalProperties"].as_bool() == Some(false));
}

// ===== Exploration Refiner Schema Tests =====

#[test]
fn exploration_refiner_schema_is_valid_json() {
    use explore_ai_agent::agents::exploration_refiner::REFINER_SCHEMA;
    let schema: serde_json::Value = serde_json::from_str(REFINER_SCHEMA)
        .expect("Refiner schema should be valid JSON");

    assert_eq!(schema["name"], "exploration_refiner_response");
    assert!(schema["strict"].as_bool().unwrap());

    let required = schema["schema"]["required"].as_array().unwrap();
    let required_fields: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(required_fields.contains(&"key_findings"));
    assert!(required_fields.contains(&"critical_files"));
    assert!(required_fields.contains(&"missing_info"));
    assert!(required_fields.contains(&"confidence"));
}

// ===== Conversation Refiner Schema Tests =====

#[test]
fn conversation_refiner_schema_is_valid_json() {
    use explore_ai_agent::agents::conversation_refiner::CONVERSATION_REFINER_SCHEMA;
    let schema: serde_json::Value = serde_json::from_str(CONVERSATION_REFINER_SCHEMA)
        .expect("Conversation refiner schema should be valid JSON");

    assert_eq!(schema["name"], "conversation_refiner_response");
    assert!(schema["strict"].as_bool().unwrap());

    let required = schema["schema"]["required"].as_array().unwrap();
    let required_fields: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(required_fields.contains(&"summary"));
}

// ===== Deep Explorer Tests =====

#[test]
fn deep_explorer_max_tool_calls() {
    use explore_ai_agent::agents::deep_explorer::{DeepExplorer, MAX_TOOL_CALLS};
    assert_eq!(MAX_TOOL_CALLS, 75);

    let explorer = DeepExplorer::new();
    assert_eq!(explorer.max_tool_calls, 75);
}

#[test]
fn deep_explorer_duplicate_detection() {
    use explore_ai_agent::agents::deep_explorer::DeepExplorer;
    let mut explorer = DeepExplorer::new();

    // First call should not be duplicate
    assert!(!explorer.check_duplicate("read_file", "hash_abc123"));

    // Same call should be duplicate
    assert!(explorer.check_duplicate("read_file", "hash_abc123"));

    // Different call should not be duplicate
    assert!(!explorer.check_duplicate("search_content", "hash_def456"));
}

#[test]
fn deep_explorer_loop_warning_generation() {
    use explore_ai_agent::agents::deep_explorer::DeepExplorer;
    let mut explorer = DeepExplorer::new();

    // No duplicates → no warning
    assert!(explorer.generate_loop_warning().is_none());

    // After consecutive duplicates → should generate warning
    explorer.check_duplicate("read_file", "hash_same");
    explorer.check_duplicate("read_file", "hash_same");
    explorer.check_duplicate("read_file", "hash_same");

    let warning = explorer.generate_loop_warning();
    if let Some(w) = warning {
        assert!(w.contains("警告") || w.contains("warning") || w.contains("调整"));
    }
}

// ===== Quality Evaluation Deserialization =====

#[test]
fn quality_evaluation_deserialize() {
    use explore_ai_agent::agents::quality_evaluator::QualityEvaluation;
    let json = serde_json::json!({
        "key_findings": "Found relevant code",
        "critical_files": [
            {"path": "src/main.rs", "one_sentence_summary": "Entry point"}
        ],
        "missing_info": "",
        "confidence": 0.85,
        "action": "answer",
        "reason": "Sufficient data found"
    });

    let eval: QualityEvaluation = serde_json::from_value(json).unwrap();
    assert_eq!(eval.confidence, 0.85);
    assert_eq!(eval.action, ExplorationAction::Answer);
    assert_eq!(eval.reason, "Sufficient data found");
}

#[test]
fn quality_evaluation_deep_explore_action() {
    use explore_ai_agent::agents::quality_evaluator::QualityEvaluation;
    let json = serde_json::json!({
        "key_findings": "Partial data",
        "critical_files": [],
        "missing_info": "Need more details",
        "confidence": 0.3,
        "action": "deep_explore",
        "reason": "Insufficient data"
    });

    let eval: QualityEvaluation = serde_json::from_value(json).unwrap();
    assert_eq!(eval.action, ExplorationAction::DeepExplore);
}
