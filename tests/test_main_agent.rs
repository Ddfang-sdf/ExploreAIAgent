use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, UnifiedResponse};
use explore_ai_agent::agents::main_agent::*;
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

fn make_exploration_data() -> serde_json::Value {
    serde_json::json!({
        "key_findings": "BooleanValidator 支持 required 和 defaultValue 两个参数",
        "critical_files": [
            {"path": "core/BooleanValidator.java", "one_sentence_summary": "核心校验逻辑"}
        ],
        "missing_info": "",
        "confidence": 0.85
    })
}

// ============================================================================
// 7.2 构造测试 (MA-001)
// ============================================================================

// MA-001: 构造 MainAgent
#[test]
fn ma_001_constructor_does_not_panic() {
    let agent = MainAgent::new();
    let _ = agent;
}

// ============================================================================
// 7.3 Prompt 组装测试 (MA-002 ~ MA-004)
// ============================================================================

// MA-002: 指令文本含角色定义
#[test]
fn ma_002_instructions_contains_role() {
    let instructions = MainAgent::assemble_prompt();
    assert!(
        instructions.contains("WSF 技术专家"),
        "指令文本应包含角色定义"
    );
}

// MA-003: 指令文本含工作要求
#[test]
fn ma_003_instructions_contains_requirements() {
    let instructions = MainAgent::assemble_prompt();
    assert!(
        instructions.contains("仅基于提供的探索数据回答"),
        "应含'仅基于提供的探索数据回答'"
    );
    assert!(
        instructions.contains("如实告知用户"),
        "应含'如实告知用户'"
    );
}

// MA-004: 指令文本含输出格式说明
#[test]
fn ma_004_instructions_contains_output_format() {
    let instructions = MainAgent::assemble_prompt();
    assert!(
        instructions.contains("<final_response>"),
        "指令文本应含 <final_response> 标签说明"
    );
}

// ============================================================================
// 7.4 集成测试 (MA-005 ~ MA-008)
// ============================================================================

// MA-005: 正常回答生成
// 推导链：mock 返回 text → generate_answer 提取 → 返回 Ok(text)
#[tokio::test]
async fn ma_005_normal_answer_generation() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(
        "<final_response>\nBooleanValidator 支持两个参数...\n</final_response>",
    ));

    let result = agent
        .generate_answer(
            "BooleanValidator 有哪些参数？",
            "",
            &make_exploration_data(),
            &mock,
        )
        .await;
    assert!(result.is_ok(), "正常回答应返回 Ok");
    let answer = result.unwrap();
    assert!(answer.contains("<final_response>"), "答案应含 <final_response> 标签");
    assert!(answer.contains("BooleanValidator"), "答案应含问题相关内容");
}

// MA-006: 数据不足如实告知
#[tokio::test]
async fn ma_006_insufficient_data_honest_answer() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(
        "<final_response>\n当前探索数据不足，无法完整回答该问题。\n</final_response>",
    ));

    let result = agent
        .generate_answer("test", "", &make_exploration_data(), &mock)
        .await;
    assert!(result.is_ok(), "应返回 Ok");
    let answer = result.unwrap();
    assert!(answer.contains("数据不足"), "答案应如实告知数据不足");
}

// MA-007: 空响应错误
// 推导链：mock 返回 text=None → generate_answer 检测 → 返回 Err("Empty response")
#[tokio::test]
async fn ma_007_empty_response_returns_err() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(Ok(UnifiedResponse {
        text: None,
        tool_calls: vec![],
    }));

    let result = agent
        .generate_answer("test", "", &make_exploration_data(), &mock)
        .await;
    assert!(result.is_err(), "空响应应返回 Err");
    let err = result.unwrap_err();
    assert!(err.contains("Empty response"), "错误信息应含 'Empty response'");
}

// MA-008: 含对话上下文的回答
#[tokio::test]
async fn ma_008_answer_with_conversation_context() {
    let agent = MainAgent::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(
        "<final_response>\n根据上一轮讨论，BooleanValidator 的 required 参数默认为 true。\n</final_response>",
    ));

    let result = agent
        .generate_answer(
            "它默认是 true 还是 false？",
            "第1轮讨论了 BooleanValidator 的参数配置",
            &make_exploration_data(),
            &mock,
        )
        .await;
    assert!(result.is_ok(), "应返回 Ok");
    let answer = result.unwrap();
    assert!(answer.contains("默认"), "答案应包含指代消解后的回答");
}
