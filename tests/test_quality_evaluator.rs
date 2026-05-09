use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, UnifiedResponse};
use explore_ai_agent::agents::quality_evaluator::*;
use std::sync::Mutex;

struct MockLlmClient {
    response: Mutex<Option<Result<UnifiedResponse, String>>>,
}

impl MockLlmClient {
    fn new() -> Self { MockLlmClient { response: Mutex::new(None) } }
    fn set_response(&self, r: Result<UnifiedResponse, String>) { *self.response.lock().unwrap() = Some(r); }
}

#[async_trait::async_trait]
impl LlmStructuredClient for MockLlmClient {
    async fn call_llm_structured(&self, _i: &str, _d: &serde_json::Value, _s: Option<&serde_json::Value>) -> Result<UnifiedResponse, String> {
        self.response.lock().unwrap().take().expect("Mock called without preset response")
    }
}

fn mock_text(text: &str) -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse { text: Some(text.to_string()), tool_calls: vec![], reasoning: None })
}

fn make_qe_json(confidence: f64) -> String {
    format!(r#"{{"key_findings":"发现","critical_files":[],"missing_info":"","confidence":{}}}"#, confidence)
}

fn make_exploration_data() -> serde_json::Value {
    serde_json::json!({"total_matches": 3, "top_matches": [{"file":"src/main.rs","line":"1","content":"fn main()"}]})
}

// ============================================================================
// 8.2.1 Schema (QE-001 ~ QE-003)
// ============================================================================

#[test]
fn qe_001_schema_valid_json() {
    let s = ExplorationQualityEvaluator::output_schema();
    let v: serde_json::Value = serde_json::from_str(s).unwrap();
    assert!(v.get("name").is_some());
    assert!(v.get("strict").is_some());
    assert!(v.get("schema").is_some());
}

#[test]
fn qe_002_schema_has_4_required_fields() {
    let s = ExplorationQualityEvaluator::output_schema();
    let v: serde_json::Value = serde_json::from_str(s).unwrap();
    let req = v["schema"]["required"].as_array().unwrap();
    let fields: Vec<&str> = req.iter().filter_map(|f| f.as_str()).collect();
    assert!(fields.contains(&"key_findings"));
    assert!(fields.contains(&"critical_files"));
    assert!(fields.contains(&"missing_info"));
    assert!(fields.contains(&"confidence"));
}

#[test]
fn qe_003_schema_no_action_field() {
    let s = ExplorationQualityEvaluator::output_schema();
    let v: serde_json::Value = serde_json::from_str(s).unwrap();
    let req = v["schema"]["required"].as_array().unwrap();
    let fields: Vec<&str> = req.iter().filter_map(|f| f.as_str()).collect();
    assert!(!fields.contains(&"action"), "v1.2 schema 不应含 action");
    assert!(!fields.contains(&"reason"), "v1.2 schema 不应含 reason");
}

// ============================================================================
// 8.2.2 Prompt (QE-004 ~ QE-005)
// ============================================================================

#[test]
fn qe_004_prompt_has_confidence_table() {
    let p = ExplorationQualityEvaluator::assemble_instructions();
    assert!(p.contains("置信度"), "Prompt 应含评分标准");
}

#[test]
fn qe_005_prompt_no_action_guidance() {
    let p = ExplorationQualityEvaluator::assemble_instructions();
    assert!(!p.contains("action"), "v1.2 不应含 action 引导");
    assert!(!p.contains("deep_explore"), "v1.2 不应含 deep_explore");
}

// ============================================================================
// 8.3.1 Normal (QE-010 ~ QE-012)
// ============================================================================

#[tokio::test]
async fn qe_010_normal_evaluation() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(&make_qe_json(0.8)));

    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_ok());
    assert_eq!(r.unwrap().confidence, 0.8);
}

#[tokio::test]
async fn qe_011_empty_exploration_data() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(&make_qe_json(0.0)));
    let r = qe.evaluate("test", &serde_json::json!({"total_matches": 0}), &mock).await;
    assert!(r.is_ok());
}

#[tokio::test]
async fn qe_012_large_data_truncated() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(&make_qe_json(0.5)));
    // data truncation is code-layer responsibility, QE just receives truncated data
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_ok());
}

// ============================================================================
// 8.3.2 Error (QE-020 ~ QE-024)
// ============================================================================

#[tokio::test]
async fn qe_020_confidence_above_range() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(&make_qe_json(1.5)));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_err());
    assert!(r.unwrap_err().contains("confidence out of range"));
}

#[tokio::test]
async fn qe_021_confidence_negative() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(&make_qe_json(-0.1)));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_err());
    assert!(r.unwrap_err().contains("confidence out of range"));
}

#[tokio::test]
async fn qe_022_invalid_json() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text("not json"));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_err());
}

#[tokio::test]
async fn qe_023_empty_response() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(Ok(UnifiedResponse { text: None, tool_calls: vec![], reasoning: None }));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_err());
}

#[tokio::test]
async fn qe_024_missing_required_fields() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(r#"{"confidence": 0.5}"#));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_err());
}

// ============================================================================
// 8.3.3 Boundary (QE-040 ~ QE-043)
// ============================================================================

#[tokio::test]
async fn qe_040_confidence_zero() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(&make_qe_json(0.0)));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_ok());
    assert_eq!(r.unwrap().confidence, 0.0);
}

#[tokio::test]
async fn qe_041_confidence_one() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(&make_qe_json(1.0)));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_ok());
    assert_eq!(r.unwrap().confidence, 1.0);
}

#[tokio::test]
async fn qe_042_empty_key_findings() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(r#"{"key_findings":"","critical_files":[],"missing_info":"","confidence":0.5}"#));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_ok());
}

#[tokio::test]
async fn qe_043_empty_critical_files() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text(r#"{"key_findings":"x","critical_files":[],"missing_info":"","confidence":0.5}"#));
    let r = qe.evaluate("test", &make_exploration_data(), &mock).await;
    assert!(r.is_ok());
}
