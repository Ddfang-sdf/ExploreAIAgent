use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, UnifiedResponse};
use explore_ai_agent::agents::conversation_refiner::*;
use std::sync::Mutex;

// ============================================================================
// Mock LLM client
// ============================================================================

struct MockLlmClient {
    response: Mutex<Option<Result<UnifiedResponse, String>>>,
}

impl MockLlmClient {
    fn new() -> Self {
        MockLlmClient {
            response: Mutex::new(None),
        }
    }

    fn set_response(&self, response: Result<UnifiedResponse, String>) {
        *self.response.lock().unwrap() = Some(response);
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
        self.response
            .lock()
            .unwrap()
            .take()
            .expect("MockLlmClient: call_llm_structured called without a preset response")
    }
}

fn mock_text_response(text: &str) -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse {
        text: Some(text.to_string()),
        tool_calls: vec![],
    })
}

fn make_records() -> Vec<ConversationRoundRecord> {
    vec![
        ConversationRoundRecord {
            round: 1,
            user_question: "BooleanValidator 有哪些参数？".to_string(),
            answer_summary: "介绍了 required 和 defaultValue 两个参数。".to_string(),
            topic: "BooleanValidator 参数配置".to_string(),
        },
        ConversationRoundRecord {
            round: 2,
            user_question: "required 参数默认是 true 还是 false？".to_string(),
            answer_summary: "确认 required 参数默认为 true。".to_string(),
            topic: "BooleanValidator 参数默认值".to_string(),
        },
        ConversationRoundRecord {
            round: 3,
            user_question: "它的默认值是什么？".to_string(),
            answer_summary: "说明了 defaultValue 的装载机制。".to_string(),
            topic: "defaultValue 默认值".to_string(),
        },
    ]
}

// ============================================================================
// 8.2 数据结构测试 (CR-001 ~ CR-002)
// ============================================================================

// CR-001: ConversationRoundRecord 序列化往返
#[test]
fn cr_001_conversation_round_record_roundtrip() {
    let original = ConversationRoundRecord {
        round: 3,
        user_question: "它有哪些参数？".to_string(),
        answer_summary: "说明了 required 和 defaultValue。".to_string(),
        topic: "参数配置".to_string(),
    };

    let json = serde_json::to_string(&original).expect("序列化失败");
    let restored: ConversationRoundRecord = serde_json::from_str(&json).expect("反序列化失败");

    assert_eq!(restored.round, 3);
    assert_eq!(restored.user_question, "它有哪些参数？");
    assert_eq!(restored.answer_summary, "说明了 required 和 defaultValue。");
    assert_eq!(restored.topic, "参数配置");
}

// CR-002: ConversationRefinerOutput 序列化往返
#[test]
fn cr_002_conversation_refiner_output_roundtrip() {
    let original = ConversationRefinerOutput {
        summary: "第1-2轮讨论了基本用法，第3轮追问了参数配置。".to_string(),
    };

    let json = serde_json::to_string(&original).expect("序列化失败");
    let restored: ConversationRefinerOutput = serde_json::from_str(&json).expect("反序列化失败");

    assert_eq!(restored.summary, original.summary);
}

// ============================================================================
// 8.3 构造与 Schema 测试 (CR-003 ~ CR-007)
// ============================================================================

// CR-003: 构造精炼专家
#[test]
fn cr_003_constructor_does_not_panic() {
    let agent = ConversationRefinerAgent::new();
    let _ = agent;
}

// CR-004: output_schema 返回合法 JSON
#[test]
fn cr_004_output_schema_returns_valid_json() {
    let schema_str = ConversationRefinerAgent::output_schema();
    let schema: serde_json::Value =
        serde_json::from_str(schema_str).expect("output_schema 不是合法的 JSON");

    assert!(schema.get("name").is_some(), "缺少 name 字段");
    assert!(schema.get("strict").is_some(), "缺少 strict 字段");
    assert!(schema.get("schema").is_some(), "缺少 schema 字段");
}

// CR-005: output_schema 的 name 字段
#[test]
fn cr_005_output_schema_name_field() {
    let schema_str = ConversationRefinerAgent::output_schema();
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    assert_eq!(
        schema["name"].as_str().unwrap(),
        "conversation_refiner_response"
    );
}

// CR-006: output_schema 含唯一 required 字段 summary
#[test]
fn cr_006_output_schema_required_summary_only() {
    let schema_str = ConversationRefinerAgent::output_schema();
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    let required = schema["schema"]["required"]
        .as_array()
        .expect("required 应该是数组");

    assert_eq!(required.len(), 1, "应仅有 1 个 required 字段");
    assert_eq!(required[0].as_str().unwrap(), "summary");
}

// CR-007: output_schema 的 strict 为 true
#[test]
fn cr_007_output_schema_strict_is_true() {
    let schema_str = ConversationRefinerAgent::output_schema();
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    assert_eq!(schema["strict"].as_bool().unwrap(), true);
}

// ============================================================================
// 8.4 Prompt 组装测试 (CR-008 ~ CR-010)
// ============================================================================

// CR-008: 指令文本含角色定义
#[test]
fn cr_008_instructions_contains_role() {
    let instructions = ConversationRefinerAgent::assemble_prompt();
    assert!(
        instructions.contains("对话上下文精炼专家"),
        "指令文本应包含角色定义"
    );
}

// CR-009: 指令文本含四类精炼要求
#[test]
fn cr_009_instructions_contains_refinement_rules() {
    let instructions = ConversationRefinerAgent::assemble_prompt();
    assert!(instructions.contains("保留话题演变"), "应含保留话题演变");
    assert!(instructions.contains("保留指代关系"), "应含保留指代关系");
    assert!(instructions.contains("去除冗余"), "应含去除冗余");
    assert!(instructions.contains("长度控制"), "应含长度控制");
}

// CR-010: 指令文本含输出格式与示例
#[test]
fn cr_010_instructions_contains_output_format() {
    let instructions = ConversationRefinerAgent::assemble_prompt();
    assert!(instructions.contains("\"summary\""), "应含 summary 字段名说明");
    assert!(
        instructions.contains("示例输出") || instructions.contains("示例"),
        "应含示例输出章节"
    );
}

// ============================================================================
// 8.5 集成测试 (CR-011 ~ CR-014)
// ============================================================================

// CR-011: 正常精炼流程（含指代关系消解）
// 推导链：mock 返回 JSON {summary:"..."} → refine() 反序列化 → Ok
#[tokio::test]
async fn cr_011_normal_refinement_with_anaphora() {
    let agent = ConversationRefinerAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(
        r#"{"summary":"第1-2轮讨论了BooleanValidator用法，第3轮追问参数。当前'它'指代required参数。"}"#,
    ));

    let result = agent
        .refine("它的默认值是什么？", &make_records(), "已有摘要文本", &mock)
        .await;
    assert!(result.is_ok(), "正常精炼应返回 Ok");
    let output = result.unwrap();
    assert!(output.summary.contains("指代"), "摘要应含指代关系消解");
}

// CR-012: 空历史摘要（首次精炼）
#[tokio::test]
async fn cr_012_first_refinement_empty_summary() {
    let agent = ConversationRefinerAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(
        r#"{"summary":"第1-3轮讨论了BooleanValidator的参数配置。"}"#,
    ));

    let result = agent
        .refine("它的默认值是什么？", &make_records(), "", &mock)
        .await;
    assert!(result.is_ok(), "首次精炼应返回 Ok");
    let output = result.unwrap();
    assert!(!output.summary.is_empty(), "summary 不应为空");
}

// CR-013: 空响应错误
// 推导链：mock 返回 text=None → refine() 检测 → Err("Empty response")
#[tokio::test]
async fn cr_013_empty_response_returns_err() {
    let agent = ConversationRefinerAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(Ok(UnifiedResponse {
        text: None,
        tool_calls: vec![],
    }));

    let result = agent
        .refine("test", &make_records(), "summary", &mock)
        .await;
    assert!(result.is_err(), "空响应应返回 Err");
    let err = result.unwrap_err();
    assert!(err.contains("Empty response"), "错误信息应含 'Empty response'");
}

// CR-014: LLM 返回非法 JSON
// 推导链：mock 返回 text="not valid json" → refine() JSON 反序列化失败 → Err
#[tokio::test]
async fn cr_014_invalid_json_returns_err() {
    let agent = ConversationRefinerAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response("not valid json"));

    let result = agent
        .refine("test", &make_records(), "summary", &mock)
        .await;
    assert!(result.is_err(), "非法 JSON 应返回 Err");
    let err = result.unwrap_err();
    assert!(err.contains("Failed to parse"), "错误信息应含 'Failed to parse'");
}
