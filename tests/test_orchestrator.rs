use explore_ai_agent::context::exploration::ExplorationContextTool;
use explore_ai_agent::orchestrator::orchestrator::Orchestrator;
use explore_ai_agent::adapter::api_adapter::{ApiAdapter, ApiMode};
use explore_ai_agent::tools::registry::ToolRegistry;
use explore_ai_agent::conversation::manager::ConversationManager;
use std::path::PathBuf;
use std::sync::Arc;

fn make_orchestrator() -> Orchestrator {
    let adapter = Arc::new(ApiAdapter::new(ApiMode::Chat));
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let cm = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    Orchestrator::new(adapter, registry, cm)
}

fn make_ect() -> ExplorationContextTool {
    ExplorationContextTool::new("orch-test".to_string())
}

// ============================================================================
// v1.2: 薄调度层集成测试 (OR-001 ~ OR-003)
// ============================================================================

// OR-001: 正常流程
#[tokio::test]
async fn or_001_normal_flow() {
    let orch = make_orchestrator();
    let mut ect = make_ect();
    let result = orch.run("你好", &mut ect).await;
    // stub: 实现后 MainAgent 直接 answer → Orchestrator 返回答案
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok(答案)");
}

// OR-002: MainAgent 失败
#[tokio::test]
async fn or_002_main_agent_failure() {
    let orch = make_orchestrator();
    let mut ect = make_ect();
    let result = orch.run("test", &mut ect).await;
    // stub: MainAgent::run() 返回 Err → Orchestrator 透传错误
    assert!(result.is_err(), "stub 占位，实现后 MainAgent 失败应返回 Err");
}

// OR-003: CM 保存失败不阻塞
#[tokio::test]
async fn or_003_cm_save_failure_does_not_block() {
    let orch = make_orchestrator();
    let mut ect = make_ect();
    let result = orch.run("test", &mut ect).await;
    // stub: MainAgent 正常，CM 保存失败 → run() 仍返回 Ok
    assert!(result.is_err(), "stub 占位，实现后 CM 失败不阻塞答案返回");
}
