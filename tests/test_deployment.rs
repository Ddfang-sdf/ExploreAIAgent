use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use explore_ai_agent::adapter::api_adapter::{ApiAdapter, ApiMode};
use explore_ai_agent::cli;
use explore_ai_agent::common::config::{AppConfig, DeepExplorerConfig};
use explore_ai_agent::conversation::manager::ConversationManager;
use explore_ai_agent::orchestrator::orchestrator::Orchestrator;
use explore_ai_agent::tools::registry::ToolRegistry;
use explore_ai_agent::web::{handle_chat_request, health_response, AppState, ChatRequest};

// ============================================================================
// Helpers
// ============================================================================

fn make_config() -> AppConfig {
    serde_json::from_str("{}").expect("默认配置解析失败")
}

fn make_adapter() -> Arc<ApiAdapter> {
    Arc::new(ApiAdapter::new(ApiMode::Chat))
}

fn make_registry() -> Arc<ToolRegistry> {
    Arc::new(ToolRegistry::new(PathBuf::from(".")))
}

fn make_conversation_manager() -> ConversationManager {
    ConversationManager::new(ApiAdapter::new(ApiMode::Chat))
}

fn make_orchestrator() -> Orchestrator {
    Orchestrator::new(make_adapter(), make_registry(), make_conversation_manager(), DeepExplorerConfig::default())
}

fn make_app_state() -> AppState {
    AppState::new(
        make_orchestrator(),
        make_conversation_manager(),
        make_config(),
    )
}

// ============================================================================
// 6.2 CLI 测试 (DP-001 ~ DP-004)
// ============================================================================

// DP-001: 正常问答
// 推导链：stdin="test question\n/exit\n" → REPL 循环 → Orchestrator::run → stdout 含回答
#[tokio::test]
async fn dp_001_normal_qa() {
    let config = make_config();
    let input = Cursor::new("test question\n/exit\n");
    let mut output = Vec::new();

    let result = cli::run_cli_with_io(&config, input, &mut output).await;
    assert!(result.is_ok(), "REPL 正常退出应返回 Ok");
    // 实现后 output 应含 mock 预设回答
}

// DP-002: 空输入跳过
// 推导链：stdin="\n/exit\n" → REPL 读到空行 → 跳过 → /exit → Ok
#[tokio::test]
async fn dp_002_empty_input_skipped() {
    let config = make_config();
    let input = Cursor::new("\n/exit\n");
    let mut output = Vec::new();

    let result = cli::run_cli_with_io(&config, input, &mut output).await;
    assert!(result.is_ok(), "REPL 正常退出应返回 Ok");
}

// DP-003: 退出命令
// 推导链：stdin="/exit\n" → REPL 识别退出命令 → 退出循环 → Ok
#[tokio::test]
async fn dp_003_exit_command() {
    let config = make_config();
    let input = Cursor::new("/exit\n");
    let mut output = Vec::new();

    let result = cli::run_cli_with_io(&config, input, &mut output).await;
    assert!(result.is_ok(), "REPL 正常退出应返回 Ok");
}

// DP-004: 多轮对话
// 推导链：stdin="Q1\nQ2\n/exit\n" → REPL 循环 2 次 → stdout 含两次回答
#[tokio::test]
async fn dp_004_multi_round() {
    let config = make_config();
    let input = Cursor::new("Q1\nQ2\n/exit\n");
    let mut output = Vec::new();

    let result = cli::run_cli_with_io(&config, input, &mut output).await;
    assert!(result.is_ok(), "REPL 正常退出应返回 Ok");
}

// ============================================================================
// 6.3 Web 测试 (DP-005 ~ DP-009)
// ============================================================================

// DP-005: 正常问答（首次请求无 session_id）
// 推导链：ChatRequest{question:"test"} → handle_chat_request → ChatResponse
// stub 阶段：返回 ChatResponse{code:5}，验证方法可调用且返回正确类型
#[tokio::test]
async fn dp_005_first_request_no_session_id() {
    let state = make_app_state();
    let body = ChatRequest {
        session_id: None,
        question: "test question".to_string(),
    };

    let response = handle_chat_request(body, &state).await;
    // orchestrator.run 返回 Err → code=5（内部错误）；实现后应为 code=0
    assert!(!response.session_id.is_empty(), "session_id 应自动生成");
    assert!(response.code != 0, "orchestrator stub 阶段 code≠0，实现后应为 code=0");
}

// DP-006: 多轮对话（携带 session_id）
// 推导链：ChatRequest{session_id:"s1", question:"Q2"} → handle_chat_request → ChatResponse
// stub 阶段：验证响应类型正确，session_id 字段存在
#[tokio::test]
async fn dp_006_multi_round_with_session_id() {
    let state = make_app_state();
    let body = ChatRequest {
        session_id: Some("s1".to_string()),
        question: "Q2".to_string(),
    };

    let response = handle_chat_request(body, &state).await;
    // stub 阶段空；实现后应为 "s1"
    assert_eq!(response.session_id, "s1", "携带 session_id 时应保留原值");
}

// DP-007: question 为空
// 推导链：ChatRequest{question:""} → handle_chat_request 校验 → ChatResponse{code:2}
// stub 阶段：返回 code=5；实现后应为 code=2
#[tokio::test]
async fn dp_007_empty_question_rejected() {
    let state = make_app_state();
    let body = ChatRequest {
        session_id: None,
        question: "".to_string(),
    };

    let response = handle_chat_request(body, &state).await;
    // 实现后 question 为空应返回 code=2
    assert_eq!(response.code, 2, "空 question 应返回 code=2");
    assert!(response.error.is_some(), "应包含错误信息");
}

// DP-008: 非法 JSON
// 推导链：非法 JSON 字符串 → axum 的 Json extractor 在进入 handle_chat_request 前拦截
// 此测试验证 ChatRequest 的 serde 边界：非法 JSON 无法反序列化
#[test]
fn dp_008_invalid_json_rejected() {
    let result: Result<ChatRequest, _> = serde_json::from_str("not json");
    assert!(result.is_err(), "非法 JSON 应反序列化失败");
}

// DP-009: 健康检查
// 推导链：GET /health → health_response() → {"status":"ok"}
#[test]
fn dp_009_health_check() {
    let response = health_response();
    assert_eq!(response["status"], "ok");
}
