//! End-to-end: real shell commands against a realistic project directory.

use async_trait::async_trait;
use explore_ai_agent::common::config::AppConfig;
use explore_ai_agent::orchestrator::orchestrator::{Orchestrator, ShellExec};
use explore_ai_agent::agents::main_agent::ShellExecutor;
use explore_ai_agent::tools::execute_shell::ExecuteShellTool;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::registry::ToolRegistry;
use explore_ai_agent::adapter::api_adapter::{ApiAdapter, ApiMode};
use explore_ai_agent::conversation::manager::ConversationManager;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn make_project() -> (tempfile::TempDir, ToolRegistry) {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn add() {}").unwrap();
    std::fs::write(root.join("utils.rs"), "pub fn util() {}").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    std::fs::write(root.join("config.toml"), "key = val").unwrap();
    std::fs::write(root.join("README.md"), "# Project").unwrap();
    std::fs::write(root.join("CHANGELOG.md"), "## v1.0").unwrap();
    std::fs::write(root.join("test_main.py"), "def test(): pass").unwrap();
    std::fs::write(root.join("helper.py"), "def help(): pass").unwrap();
    std::fs::write(root.join("script.sh"), "#!/bin/bash\necho hi").unwrap();
    std::fs::write(root.join("Dockerfile"), "FROM rust").unwrap();
    std::fs::write(root.join("Makefile"), "all:").unwrap();
    std::fs::write(root.join(".gitignore"), "target/").unwrap();
    std::fs::write(root.join("LICENSE"), "MIT").unwrap();

    std::fs::create_dir_all(root.join("src/models")).unwrap();
    std::fs::write(root.join("src/models/user.rs"), "pub struct User;").unwrap();
    std::fs::write(root.join("src/models/mod.rs"), "pub mod user;").unwrap();

    let registry = ToolRegistry::new(root.to_path_buf());
    (tmp, registry)
}

/// E2E-R1: "列出项目中所有的文件类型，每种类型有多少个文件"
#[tokio::test]
async fn e2e_r1_count_file_extensions() {
    let (_tmp, registry) = make_project();
    let exec = ShellExec { registry: Arc::new(registry) };

    let result = exec.execute(
        r"ls -1 | grep '\.' | awk -F. '{print $NF}' | sort | uniq -c"
    ).await;

    let data = result.expect("shell execution must succeed");
    let output = data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    let success = data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    assert!(success, "command must succeed, error: {:?}", data.get("error"));

    // Verify each extension count
    assert!(output.contains("2 md"), "must have 2 .md files\ngot:\n{}", output);
    assert!(output.contains("2 py"), "must have 2 .py files\ngot:\n{}", output);
    assert!(output.contains("3 rs"), "must have 3 .rs files\ngot:\n{}", output);
    assert!(output.contains("1 sh"), "must have 1 .sh file\ngot:\n{}", output);
    assert!(output.contains("2 toml"), "must have 2 .toml files\ngot:\n{}", output);

    // No extension files (Dockerfile, Makefile, LICENSE) should NOT appear
    let no_ext = ["Dockerfile", "Makefile", "LICENSE"];
    for f in &no_ext {
        assert!(!output.contains(f), "{} must NOT appear (no extension)\ngot:\n{}", f, output);
    }
}

/// E2E-R2: diagnose pipe execution on real shell
#[tokio::test]
async fn e2e_r2_diagnose_pipes() {
    let (_tmp, registry) = make_project();
    let root = _tmp.path();

    // Go through the full tool chain, not just ShellExec (which wraps ToolRegistry)
    let tool = ExecuteShellTool::new(root.to_path_buf());
    eprintln!("shell_path={}", tool.shell_path);

    // Test 1: simple pipe
    let input = ToolInput {
        tool_name: "execute_shell".to_string(),
        params: serde_json::json!({"command": "echo hello | grep hello"}),
        project_root: root.to_path_buf(),
    };
    let result = tool.execute(input).unwrap();
    let output: serde_json::Value = result.data;
    eprintln!("test1: success={} output='{}'", result.success, output["output"]);

    // Test 2: ls piped through grep
    let input2 = ToolInput {
        tool_name: "execute_shell".to_string(),
        params: serde_json::json!({"command": "ls -1 | grep '\\.'"}),
        project_root: root.to_path_buf(),
    };
    let result2 = tool.execute(input2).unwrap();
    let output2: serde_json::Value = result2.data;
    eprintln!("test2: success={} has_dotless_files={}",
        result2.success,
        output2["output"].as_str().unwrap_or("").contains("LICENSE"));

    assert!(!output2["output"].as_str().unwrap_or("").contains("LICENSE"),
        "grep should filter out dotless files");
}

// ============================================================================
// E2E-R3: DE 可配置开关 — 端到端验证
// ============================================================================

/// E2E-R3: 打印 Shell 检测结果
#[test]
fn e2e_r3_shell_detection_dump() {
    use explore_ai_agent::tools::execute_shell::ExecuteShellTool;
    use explore_ai_agent::agents::main_agent::MainAgent;
    eprintln!("=== Shell Detection ===");
    eprintln!("shell_info:     {}", MainAgent::shell_info());
    eprintln!("shell_commands: {}", MainAgent::shell_commands());
    let tool = ExecuteShellTool::new(std::path::PathBuf::from("."));
    eprintln!("discover_shell: {}", tool.shell_path);
    eprintln!("======================");
}

/// E2E-R4: 默认配置 DE 开启
#[test]
fn e2e_r4_de_enabled_by_default() {
    let config = AppConfig::default();
    assert!(config.deep_explorer.enable,
        "DE must be enabled by default");
}

/// E2E-R4: YAML 配置 DE 禁用可正确加载
#[test]
fn e2e_r4_de_disabled_from_yaml() {
    let yaml = r#"
llm:
  api_key: "test"
deep_explorer:
  enable: false
"#;
    let config: AppConfig = serde_yaml::from_str(yaml).expect("must parse yaml");
    assert!(!config.deep_explorer.enable,
        "deep_explorer.enable: false must be parsed correctly");

    // Verify orchestrator can be constructed with disabled DE
    let adapter = Arc::new(ApiAdapter::new(ApiMode::Chat));
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let cm = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let orch = Orchestrator::from_config(
        adapter,
        registry,
        cm,
        &config.exploration,
        &config.deep_explorer,
    );
    // Orchestrator constructed successfully with DE disabled
    assert!(!orch.de_config.enable,
        "orchestrator must reflect DE disabled config");
}

// ============================================================================
// E2E-R5: fast_explore mtime sorting end-to-end
// ============================================================================

#[tokio::test]
async fn e2e_r5_fast_explore_mtime_sorting() {
    use explore_ai_agent::context::exploration::ExplorationContextTool;
    use explore_ai_agent::tools::fast_explore_tool::FastExploreTool;
    use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, UnifiedResponse};
    use std::fs::OpenOptions;

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    // Create 3 files with same keyword, different mtimes
    let old = root.join("old.rs");
    let mid = root.join("mid.rs");
    let new = root.join("new.rs");
    std::fs::write(&old, "fn search_target() {}").unwrap();
    std::fs::write(&mid, "fn search_target() {}").unwrap();
    std::fs::write(&new, "fn search_target() {}").unwrap();

    let now = std::time::SystemTime::now();
    let h = std::time::Duration::from_secs(3600);
    OpenOptions::new().write(true).open(&old).unwrap().set_modified(now - h * 3).unwrap();
    OpenOptions::new().write(true).open(&mid).unwrap().set_modified(now - h * 2).unwrap();
    OpenOptions::new().write(true).open(&new).unwrap().set_modified(now - h).unwrap();

    // Mock QE
    struct MockQe { resp: Mutex<Option<Result<UnifiedResponse, String>>> }
    impl MockQe {
        fn set(&self, r: Result<UnifiedResponse, String>) { *self.resp.lock().unwrap() = Some(r); }
    }
    #[async_trait::async_trait]
    impl LlmStructuredClient for &MockQe {
        async fn call_llm_structured(&self, _i: &str, _d: &serde_json::Value, _s: Option<&serde_json::Value>) -> Result<UnifiedResponse, String> {
            self.resp.lock().unwrap().take().unwrap()
        }
    }

    let qe = MockQe { resp: Mutex::new(None) };
    qe.set(Ok(UnifiedResponse {
        text: Some(r#"{"key_findings":"found","critical_files":[],"missing_info":"","confidence":0.9}"#.to_string()),
        tool_calls: vec![],
        reasoning: None,
    }));

    let registry = ToolRegistry::new(root.to_path_buf());
    let ect = ExplorationContextTool::new("e2e-mtime".to_string());

    let result = FastExploreTool::execute(
        &vec!["search_target".to_string()],
        &registry, &ect, &(&qe),
    ).await.expect("fast_explore must succeed");

    let matches = result.get("matches").and_then(|v| v.as_array())
        .expect("must have matches");
    assert!(matches.len() >= 3, "must find 3 files, got {}", matches.len());

    // Verify: newest file (new.rs, modified 1h ago) appears before oldest (old.rs, 3h ago)
    let first = matches[0].get("file").and_then(|v| v.as_str()).unwrap_or("");
    let last = matches[matches.len()-1].get("file").and_then(|v| v.as_str()).unwrap_or("");
    assert!(first.ends_with("new.rs"), "newest must be first, got: {}", first);
    assert!(last.ends_with("old.rs"), "oldest must be last, got: {}", last);
}
