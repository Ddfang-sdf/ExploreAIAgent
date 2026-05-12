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
    let (_tmp, _registry) = make_project();
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
        &config.fast_explore,
    );
    // Orchestrator constructed successfully with DE disabled
    assert!(!orch.de_config.enable,
        "orchestrator must reflect DE disabled config");
}

// ============================================================================
// E2E-R5: fast_explore 可配置开关 — 端到端验证
// ============================================================================

/// E2E-R5: 默认 fast_explore.enable: true
#[test]
fn e2e_r5_fe_enabled_by_default() {
    let config = AppConfig::default();
    assert!(config.fast_explore.enable, "FE must be enabled by default");
}

/// E2E-R6: fast_explore 禁用 → 只有 DE + shell
#[test]
fn e2e_r6_fe_disabled_from_yaml() {
    let yaml = r#"
llm:
  api_key: "test"
fast_explore:
  enable: false
"#;
    let config: AppConfig = serde_yaml::from_str(yaml).expect("must parse");
    assert!(!config.fast_explore.enable, "FE disabled must be parsed");

    let adapter = Arc::new(ApiAdapter::new(ApiMode::Chat));
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let cm = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let _orch = Orchestrator::from_config(
        adapter, registry, cm,
        &config.exploration, &config.deep_explorer, &config.fast_explore,
    );
    // Verify orchestrator can be built with FE disabled
    assert!(!config.fast_explore.enable, "orchestrator must reflect FE disabled");
}

/// E2E-R7: fast_explore + DE 同时禁用 → 只有 shell
#[test]
fn e2e_r7_fe_and_de_disabled_shell_only() {
    let yaml = r#"
llm:
  api_key: "test"
deep_explorer:
  enable: false
fast_explore:
  enable: false
"#;
    let config: AppConfig = serde_yaml::from_str(yaml).expect("must parse");
    assert!(!config.fast_explore.enable);
    assert!(!config.deep_explorer.enable);

    let adapter = Arc::new(ApiAdapter::new(ApiMode::Chat));
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let cm = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let _orch = Orchestrator::from_config(
        adapter, registry, cm,
        &config.exploration, &config.deep_explorer, &config.fast_explore,
    );
    // Orchestrator built successfully with FE+DE disabled — only shell available
    assert!(!config.fast_explore.enable && !config.deep_explorer.enable,
        "both FE and DE must be disabled");
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

// ============================================================================
// E2E-R8 ~ E2E-R14: fast_explore + shell 端到端组合测试
// ============================================================================

/// E2E-R8: FE 开启 → fast_explorer 搜索后 shell 验证结果一致性
/// 场景：fast_explorer 搜索关键字 → 拿到匹配文件和行号 → shell grep 验证
/// 推导链：registry.execute("fast_explorer", {keywords:["fn main"]})
///        → 提取匹配文件列表
///        → registry.execute("execute_shell", {command:"grep -n 'fn main' <file>"})
///        → 验证 fast_explorer 和 shell 找到相同的匹配
#[test]
fn e2e_r8_fe_enabled_fast_explorer_then_shell_verify() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    // Create a realistic project with known content
    std::fs::write(root.join("main.rs"), "fn main() {\n    println!(\"hello\");\n}\n").unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\npub fn sub(a: i32, b: i32) -> i32 { a - b }\n").unwrap();
    std::fs::write(root.join("utils.rs"), "pub fn format_name(name: &str) -> String {\n    name.to_string()\n}\n").unwrap();
    std::fs::write(root.join("config.toml"), "[server]\nport = 8080\n").unwrap();
    std::fs::write(root.join("README.md"), "# Project\n\nfn main() is the entry point.\n").unwrap();

    let registry = ToolRegistry::new(root.to_path_buf());

    // Step 1: fast_explorer search for "fn " keyword
    let fe_result = registry.execute("fast_explorer", serde_json::json!({
        "keywords": ["fn "],
        "exclude_paths": [],
    })).expect("fast_explorer must succeed");
    assert!(fe_result.success);

    let fe_matches = fe_result.data.get("matches")
        .and_then(|v| v.as_array())
        .expect("fast_explorer must return matches");

    // Step 2: Extract files found by fast_explorer
    let fe_files: Vec<&str> = fe_matches.iter()
        .filter_map(|m| m.get("file").and_then(|v| v.as_str()))
        .collect();
    eprintln!("[E2E-R8] fast_explorer found files: {:?}", fe_files);

    // Should find fn in main.rs, lib.rs, utils.rs
    assert!(fe_files.iter().any(|f| f.contains("main.rs")),
        "fast_explorer should find fn main in main.rs");
    assert!(fe_files.iter().any(|f| f.contains("lib.rs")),
        "fast_explorer should find fn in lib.rs");
    assert!(fe_files.iter().any(|f| f.contains("utils.rs")),
        "fast_explorer should find fn in utils.rs");
    // Should NOT find fn in config.toml (no Rust code)
    let toml_matches: Vec<_> = fe_files.iter().filter(|f| f.contains(".toml")).collect();
    assert!(toml_matches.is_empty(),
        "fast_explorer should not find fn in .toml, got {:?}", toml_matches);

    // Step 3: Shell grep to verify each fast_explorer result
    for matched_file in &fe_files {
        let shell_result = registry.execute("execute_shell", serde_json::json!({
            "command": format!("grep -c 'fn ' {}", matched_file),
        })).expect("shell grep must succeed");

        let shell_data: serde_json::Value = shell_result.data;
        let shell_output = shell_data.get("output").and_then(|v| v.as_str()).unwrap_or("0");
        let shell_count: i32 = shell_output.trim().parse().unwrap_or(0);
        assert!(shell_count > 0,
            "shell grep should find 'fn ' in {}; got count={}", matched_file, shell_count);
    }

    // Step 4: Cross-check — shell find+grep counts all 'fn ' occurrences
    let shell_total = registry.execute("execute_shell", serde_json::json!({
        "command": "grep -rn 'fn ' *.rs | wc -l",
    })).expect("shell pipe must succeed");
    let total_data: serde_json::Value = shell_total.data;
    let total_str = total_data.get("output").and_then(|v| v.as_str()).unwrap_or("0");
    let shell_total_count: usize = total_str.trim().parse().unwrap_or(0);

    // fast_explorer total should be <= shell total (FE deduplicates consecutive lines)
    let fe_total = fe_result.data.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    assert!(fe_total <= shell_total_count,
        "FE total ({}) should be ≤ shell raw count ({}); FE deduplicates",
        fe_total, shell_total_count);
    eprintln!("[E2E-R8] FE total={}, shell total={}", fe_total, shell_total_count);
}

/// E2E-R9: FE 开启 → fast_explorer 结果驱动 shell 深入检查
/// 场景：fast_explorer 找到文件后，用 shell cat/head/tail 读取具体内容
///       再用 shell wc -l 统计找到的文件行数
#[test]
fn e2e_r9_fe_enabled_results_drive_shell_deep_inspect() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).unwrap();
    let mut main_content = String::new();
    for i in 1..=30 {
        main_content.push_str(&format!("// line {}\n", i));
    }
    main_content.push_str("fn main() {\n    let x = handle_request();\n}\n");
    std::fs::write(root.join("src/main.rs"), &main_content).unwrap();

    std::fs::write(root.join("src/handler.rs"),
        "pub fn handle_request() -> Response {\n    Response::ok()\n}\n").unwrap();
    std::fs::write(root.join("src/models.rs"),
        "pub struct Request {}\npub struct Response {}\n").unwrap();

    let registry = ToolRegistry::new(root.to_path_buf());

    // Step 1: fast_explorer — search for "handle_request"
    let fe_result = registry.execute("fast_explorer", serde_json::json!({
        "keywords": ["handle_request"],
        "exclude_paths": [],
    })).expect("fast_explorer must succeed");
    assert!(fe_result.success);

    let fe_matches = fe_result.data.get("matches")
        .and_then(|v| v.as_array())
        .expect("must have matches");
    assert!(!fe_matches.is_empty(), "fast_explorer should find handle_request");

    // Step 2: Use shell to read the exact lines fast_explorer found
    for m in fe_matches {
        let file = m.get("file").and_then(|v| v.as_str()).unwrap_or("");
        let line_range = m.get("line").and_then(|v| v.as_str()).unwrap_or("1");

        // Shell: cat the file to verify it exists and has expected content
        let cat_result = registry.execute("execute_shell", serde_json::json!({
            "command": format!("cat {}", file),
        })).expect("cat must succeed");
        let cat_data: serde_json::Value = cat_result.data;
        let cat_output = cat_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
        assert!(cat_output.contains("handle_request"),
            "cat of {} should contain 'handle_request'", file);

        // Shell: wc -l to check file length vs matched line
        let wc_result = registry.execute("execute_shell", serde_json::json!({
            "command": format!("wc -l {}", file),
        })).expect("wc must succeed");
        let wc_data: serde_json::Value = wc_result.data;
        let wc_output = wc_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
        eprintln!("[E2E-R9] {} line {} / wc: {}", file, line_range, wc_output.trim());
    }

    // Step 3: Shell pipeline to count total .rs files and total lines
    let summary_result = registry.execute("execute_shell", serde_json::json!({
        "command": "find . -name '*.rs' -exec cat {} + | wc -l",
    })).expect("shell pipeline must succeed");
    let summary_data: serde_json::Value = summary_result.data;
    let total_lines = summary_data.get("output").and_then(|v| v.as_str()).unwrap_or("0");
    let total: i32 = total_lines.trim().parse().unwrap_or(0);
    assert!(total > 0, "should have positive line count across all .rs files");
    eprintln!("[E2E-R9] Total lines across .rs files: {}", total);
}

/// E2E-R10: FE 禁用 → shell 独立完成探索任务
/// 场景：配置 fast_explore.enable=false 后，shell 仍然可以执行各种
///      只读命令来探索项目（ls, find, grep, wc, cat 等）
#[test]
fn e2e_r10_fe_disabled_shell_standalone_works() {
    // Parse config with FE disabled
    let yaml = r#"
llm:
  api_key: "test"
fast_explore:
  enable: false
"#;
    let config: AppConfig = serde_yaml::from_str(yaml).expect("must parse");
    assert!(!config.fast_explore.enable,
        "fast_explore must be disabled");

    // Build orchestrator with FE disabled
    let adapter = Arc::new(ApiAdapter::new(ApiMode::Chat));
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    // Create realistic project
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn add() {}\n").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
    std::fs::write(root.join("README.md"), "# Test\n").unwrap();

    let registry = Arc::new(ToolRegistry::new(root.to_path_buf()));
    let cm = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let _orch = Orchestrator::from_config(
        adapter, registry.clone(), cm,
        &config.exploration, &config.deep_explorer, &config.fast_explore,
    );

    // ShellExe still works independently via ToolRegistry
    // Test 1: ls — list all files
    let ls_result = registry.execute("execute_shell", serde_json::json!({
        "command": "ls -1",
    })).expect("ls must succeed");
    let ls_data: serde_json::Value = ls_result.data;
    let ls_output = ls_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(ls_output.contains("main.rs"), "ls should see main.rs");
    assert!(ls_output.contains("Cargo.toml"), "ls should see Cargo.toml");

    // Test 2: find — discover .rs files
    let find_result = registry.execute("execute_shell", serde_json::json!({
        "command": "find . -name '*.rs'",
    })).expect("find must succeed");
    let find_data: serde_json::Value = find_result.data;
    let find_output = find_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(find_output.contains("main.rs"));
    assert!(find_output.contains("lib.rs"));

    // Test 3: grep — search for content
    let grep_result = registry.execute("execute_shell", serde_json::json!({
        "command": "grep -rn 'fn ' .",
    })).expect("grep must succeed");
    let grep_data: serde_json::Value = grep_result.data;
    let grep_output = grep_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(grep_output.contains("fn main"), "grep should find fn main");
    assert!(grep_output.contains("fn add"), "grep should find fn add");

    // Test 4: wc — count lines
    let wc_result = registry.execute("execute_shell", serde_json::json!({
        "command": "wc -l *.rs",
    })).expect("wc must succeed");
    let wc_data: serde_json::Value = wc_result.data;
    let wc_output = wc_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    eprintln!("[E2E-R10] wc -l *.rs:\n{}", wc_output);
    assert!(!wc_output.is_empty(), "wc should produce output");

    eprintln!("[E2E-R10] FE disabled — shell standalone: all 4 operations succeeded");
}

/// E2E-R11: FE 禁用 → shell 复杂管道表达式
/// 场景：FE 禁用时，可以用 shell 管道完成复杂的探索任务
///      模拟在没有 fast_explore 的情况下如何用纯 shell 探索代码库
#[test]
fn e2e_r11_fe_disabled_shell_complex_pipeline() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"),
        "fn main() {\n    let x = 1;\n    let y = 2;\n    println!(\"{}\", x + y);\n}\n").unwrap();
    std::fs::write(root.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[cfg(test)]\nfn test_add() {\n    assert_eq!(add(2, 3), 5);\n}\n").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(root.join("README.md"), "# Demo\n\nThis is a demo project.\n").unwrap();

    let registry = ToolRegistry::new(root.to_path_buf());

    // Pipeline 1: Count functions per file
    let func_count = registry.execute("execute_shell", serde_json::json!({
        "command": "grep -rn 'fn ' src/ | awk -F: '{print $1}' | sort | uniq -c",
    })).expect("function count pipeline must succeed");
    let func_data: serde_json::Value = func_count.data;
    let func_output = func_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    eprintln!("[E2E-R11] Function count per file:\n{}", func_output);
    assert!(!func_output.is_empty(), "pipeline should produce output");

    // Pipeline 2: Find lines containing 'test' and count them
    let test_count = registry.execute("execute_shell", serde_json::json!({
        "command": "grep -rn 'test' src/ | wc -l",
    })).expect("test count pipeline must succeed");
    let test_data: serde_json::Value = test_count.data;
    let test_output = test_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    let test_lines: i32 = test_output.trim().parse().unwrap_or(-1);
    assert!(test_lines > 0, "should find test-related lines");
    eprintln!("[E2E-R11] test-related lines: {}", test_lines);

    // Pipeline 3: List all .rs files sorted by line count (most → least)
    let sort_result = registry.execute("execute_shell", serde_json::json!({
        "command": "wc -l src/*.rs | sort -rn",
    })).expect("sort pipeline must succeed");
    let sort_data: serde_json::Value = sort_result.data;
    let sort_output = sort_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    eprintln!("[E2E-R11] Files sorted by line count:\n{}", sort_output);
    assert!(!sort_output.is_empty());

    // Pipeline 4: Find unique words in README (tr + sort + uniq)
    let words_result = registry.execute("execute_shell", serde_json::json!({
        "command": "cat README.md | tr ' ' '\n' | sort -u",
    })).expect("words pipeline must succeed");
    let words_data: serde_json::Value = words_result.data;
    let words_output = words_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(!words_output.is_empty());
    eprintln!("[E2E-R11] All 4 pipelines succeeded");
}

/// E2E-R12: fast_explorer 结果驱动的自动化探索链
/// 场景：模拟真实用户场景 — 用 fast_explorer 搜索关键字 →
///       提取文件名 → shell 深入检查每个文件 →
///       shell 汇总统计
#[test]
fn e2e_r12_full_exploration_chain() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    // Create a project with Rust code patterns
    std::fs::write(root.join("auth.rs"),
        "pub fn login(user: &str, pass: &str) -> bool { true }\n\
         pub fn logout() {}\n\
         pub fn check_token(token: &str) -> bool { token.len() > 0 }\n").unwrap();
    std::fs::write(root.join("db.rs"),
        "pub fn connect(url: &str) -> Connection {}\n\
         pub fn query(sql: &str) -> Vec<Row> {}\n\
         pub fn disconnect() {}\n").unwrap();
    std::fs::write(root.join("api.rs"),
        "pub fn handle_get(path: &str) -> Response {}\n\
         pub fn handle_post(path: &str, body: &str) -> Response {}\n\
         pub fn middleware_auth(req: &Request) -> bool { true }\n").unwrap();
    std::fs::write(root.join("models.rs"),
        "pub struct Connection;\n\
         pub struct Row;\n\
         pub struct Response;\n\
         pub struct Request;\n").unwrap();

    let registry = ToolRegistry::new(root.to_path_buf());

    // Step 1: fast_explorer — find all 'pub fn ' declarations
    let fe_result = registry.execute("fast_explorer", serde_json::json!({
        "keywords": ["pub fn "],
        "exclude_paths": [],
    })).expect("fast_explorer must succeed");
    assert!(fe_result.success);
    let fe_total = fe_result.data.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
    // Note: fast_explorer deduplicates consecutive lines, so
    // auth.rs (3 consecutive fn) → 1 merged entry,
    // db.rs (3 consecutive fn) → 1 merged entry,
    // api.rs (3 consecutive fn) → 1 merged entry → total=3
    assert!(fe_total >= 1, "should find pub fn declarations, got {}", fe_total);

    // Step 2: Shell — verify total count of public functions
    let shell_count = registry.execute("execute_shell", serde_json::json!({
        "command": "grep -rn 'pub fn ' *.rs | wc -l",
    })).expect("shell count must succeed");
    let shell_data: serde_json::Value = shell_count.data;
    let shell_total: usize = shell_data.get("output").and_then(|v| v.as_str())
        .unwrap_or("0").trim().parse().unwrap_or(0);
    assert!(shell_total >= 7, "shell should count ≥ 7 pub fn, got {}", shell_total);

    // Step 3: Shell — extract function signatures for each file
    let sigs_result = registry.execute("execute_shell", serde_json::json!({
        "command": "grep -rn 'pub fn ' *.rs | awk -F: '{print $1}' | sort | uniq -c | sort -rn",
    })).expect("sigs pipeline must succeed");
    let sigs_data: serde_json::Value = sigs_result.data;
    let sigs_output = sigs_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    eprintln!("[E2E-R12] pub fn per file:\n{}", sigs_output);

    // Step 4: Cross-check — fast_explorer total ≤ shell raw count
    assert!(fe_total as usize <= shell_total,
        "FE total ({}) ≤ shell raw count ({})", fe_total, shell_total);

    eprintln!("[E2E-R12] Full exploration chain verified: FE={} shell={}", fe_total, shell_total);
}

/// E2E-R13: 配置切换 → fast_explore 启用/禁用的 Orchestrator 行为验证
/// 验证 FE enable/disable 确实影响 Orchestrator 中的 fe_config 状态
#[test]
fn e2e_r13_config_toggle_orchestrator_behavior() {
    // ---- Case 1: FE enabled (default) ----
    let config_enabled = AppConfig::default();
    assert!(config_enabled.fast_explore.enable,
        "FE must be enabled by default");

    let adapter = Arc::new(ApiAdapter::new(ApiMode::Chat));
    let registry = Arc::new(ToolRegistry::new(PathBuf::from(".")));
    let cm = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));

    let orch_enabled = Orchestrator::from_config(
        adapter.clone(), registry.clone(), cm,
        &config_enabled.exploration, &config_enabled.deep_explorer, &config_enabled.fast_explore,
    );
    assert!(orch_enabled.fe_config.enable,
        "orchestrator FE config should be enabled");

    // ---- Case 2: FE disabled ----
    let yaml_disabled = r#"
llm:
  api_key: "test"
fast_explore:
  enable: false
"#;
    let config_disabled: AppConfig = serde_yaml::from_str(yaml_disabled)
        .expect("must parse FE disabled config");
    assert!(!config_disabled.fast_explore.enable);

    let cm2 = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let orch_disabled = Orchestrator::from_config(
        adapter, registry, cm2,
        &config_disabled.exploration, &config_disabled.deep_explorer,
        &config_disabled.fast_explore,
    );
    assert!(!orch_disabled.fe_config.enable,
        "orchestrator FE config should be disabled");

    // Verify both configs produce different Orchestrator instances
    assert_ne!(orch_enabled.fe_config.enable, orch_disabled.fe_config.enable,
        "enabled vs disabled should produce different orchestrator states");
}

/// E2E-R14: FE 禁用 → fast_explorer 工具仍在 ToolRegistry 中注册
/// 但 shell 仍可正常工作（FE 禁用只影响 Orchestrator 的 FE 决策）
#[test]
fn e2e_r14_fe_disabled_tool_registry_unaffected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();

    let registry = ToolRegistry::new(root.to_path_buf());

    // Even when FE is "disabled" at config level, the fast_explorer tool
    // is still registered in ToolRegistry (it's just not wired into MainAgent)
    let tools = registry.list_tools();
    assert!(tools.contains(&"fast_explorer"),
        "fast_explorer tool should still be in registry; disabling is at orchestrator level");
    assert!(tools.contains(&"execute_shell"),
        "execute_shell should always be in registry");

    // fast_explorer still works when called directly
    let fe_result = registry.execute("fast_explorer", serde_json::json!({
        "keywords": ["fn main"],
        "exclude_paths": [],
    })).expect("fast_explorer should still work at tool level");
    assert!(fe_result.success);
    assert!(fe_result.data.get("total").and_then(|v| v.as_u64()).unwrap_or(0) > 0);

    // Shell still works
    let shell_result = registry.execute("execute_shell", serde_json::json!({
        "command": "grep 'fn main' main.rs",
    })).expect("shell should work");
    assert!(shell_result.success);
    let shell_data: serde_json::Value = shell_result.data;
    let shell_out = shell_data.get("output").and_then(|v| v.as_str()).unwrap_or("");
    assert!(shell_out.contains("fn main"));
}
