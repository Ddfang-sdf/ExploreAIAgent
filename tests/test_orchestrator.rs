use explore_ai_agent::common::config::{DeepExplorerConfig, FastExploreConfig};
use explore_ai_agent::context::exploration::ExplorationContextTool;
use explore_ai_agent::orchestrator::orchestrator::{Orchestrator, ShellExec};
use explore_ai_agent::agents::main_agent::ShellExecutor;
use explore_ai_agent::adapter::api_adapter::{ApiAdapter, ApiMode};
use explore_ai_agent::tools::registry::ToolRegistry;
use explore_ai_agent::conversation::manager::ConversationManager;
use std::path::PathBuf;
use std::sync::Arc;

fn make_orchestrator() -> Orchestrator {
    let adapter = Arc::new(ApiAdapter::new(ApiMode::Chat));
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let cm = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    Orchestrator::new(adapter, registry, cm, DeepExplorerConfig::default(), FastExploreConfig::default())
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
    let result = orch.run("你好", "", Arc::new(ect)).await;
    // stub: 实现后 MainAgent 直接 answer → Orchestrator 返回答案
    assert!(result.is_err(), "stub 占位，实现后应返回 Ok(答案)");
}

// OR-002: MainAgent 失败
#[tokio::test]
async fn or_002_main_agent_failure() {
    let orch = make_orchestrator();
    let mut ect = make_ect();
    let result = orch.run("test", "", Arc::new(ect)).await;
    // stub: MainAgent::run() 返回 Err → Orchestrator 透传错误
    assert!(result.is_err(), "stub 占位，实现后 MainAgent 失败应返回 Err");
}

// OR-003: CM 保存失败不阻塞
#[tokio::test]
async fn or_003_cm_save_failure_does_not_block() {
    let orch = make_orchestrator();
    let mut ect = make_ect();
    let result = orch.run("test", "", Arc::new(ect)).await;
    // stub: MainAgent 正常，CM 保存失败 → run() 仍返回 Ok
    assert!(result.is_err(), "stub 占位，实现后 CM 失败不阻塞答案返回");
}

// ============================================================================
// v1.3: ShellExec 测试 (OR-004 ~ OR-005)
// ============================================================================

// OR-004: ShellExec 构造与执行
#[tokio::test]
async fn or_004_shell_exec_normal() {
    // 输入: ShellExec{registry} → execute("find . -name '*.rs' | wc -l")
    // ShellExecutor::execute() → ToolRegistry.execute("execute_shell", params)
    // → Ok(ToolOutput{data: {"output":"...","success":true}})
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let exec = ShellExec { registry };
    let result = exec.execute("find . -name '*.rs' | wc -l").await;
    // execute_shell 返回: {success, output, error}
    assert!(result.is_ok(), "ShellExec must return Ok for allowed command");
    let data = result.unwrap();
    assert!(data.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
        "execute_shell result must have success=true");
    assert!(data.get("output").is_some(),
        "execute_shell result must have output field");
}

// OR-005: ShellExec 执行失败
#[tokio::test]
async fn or_005_shell_exec_failure() {
    // 输入: ShellExec{registry} → execute("rm -rf /")
    // ToolRegistry 拦截不允许的命令 → Err(ToolError)
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let exec = ShellExec { registry };
    let result = exec.execute("rm -rf /").await;
    // 不允许的命令应返回 Err
    assert!(result.is_err(), "ShellExec must return Err for disallowed command");
}

// ============================================================================
// v1.3: 端到端 Shell 测试 (真实 shell 执行)
// ============================================================================

#[tokio::test]
async fn or_006_shell_echo_end_to_end() {
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let exec = ShellExec { registry };
    let result = exec.execute("echo hello_from_shell").await;
    assert!(result.is_ok(), "echo must succeed, got: {:?}", result.err());
    let data = result.unwrap();
    assert!(data.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
        "success must be true");
    let output = data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(output.contains("hello_from_shell"),
        "output must contain echo text, got: {}", output);
}

#[tokio::test]
async fn or_007_shell_error_propagates_to_llm() {
    // Verify that error messages contain actionable info for the LLM
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let exec = ShellExec { registry };
    let result = exec.execute("python -c 'print(1)'").await;
    // python is not in whitelist → should fail with useful error
    assert!(result.is_err(), "disallowed command must return Err");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("whitelist") || err_msg.contains("not in") || err_msg.contains("not allowed"),
        "error must explain why command was rejected, got: {}", err_msg
    );
}

#[tokio::test]
async fn or_008_shell_execute_safe_pipeline() {
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let exec = ShellExec { registry };
    // Pipe between two allowed commands
    let result = exec.execute("echo hello | grep hello").await;
    assert!(result.is_ok(), "safe pipe must succeed, got: {:?}", result.err());
    let data = result.unwrap();
    assert!(data.get("success").and_then(|v| v.as_bool()).unwrap_or(false));
}
