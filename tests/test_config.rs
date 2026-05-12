use explore_ai_agent::common::config::*;
use std::fs;
use std::sync::Mutex;
use tempfile::TempDir;

// Serialize env-var-dependent tests to prevent cross-test pollution
static ENV_MUTEX: Mutex<()> = Mutex::new(());

fn write_config(dir: &TempDir, content: &str) -> String {
    let path = dir.path().join("config.yaml");
    fs::write(&path, content).unwrap();
    path.to_string_lossy().to_string()
}

// ============================================================================
// 6.1 配置加载测试 (CF-001 ~ CF-007)
// ============================================================================

// CF-001: 加载最小配置
// 推导链：YAML 含 api_key → 解析 → 其余字段默认值 → Ok
#[test]
fn cf_001_load_minimal_config() {
    let _guard = ENV_MUTEX.lock().unwrap();
    std::env::remove_var("EXPLORE_LLM__API_KEY");
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, "llm:\n  api_key: sk-test-key\n");

    let config = AppConfig::load(Some(&path)).expect("最小配置应加载成功");
    assert_eq!(config.llm.api_key, "sk-test-key");
    assert_eq!(config.llm.model, "deepseek-chat");
    assert_eq!(config.exploration.token_threshold, 12000);
}

// CF-002: 加载完整配置
// 推导链：完整 YAML → 解析 → 所有字段与 YAML 一致
#[test]
fn cf_002_load_full_config() {
    let _guard = ENV_MUTEX.lock().unwrap();
    std::env::remove_var("EXPLORE_LLM__MODEL");
    std::env::remove_var("EXPLORE_LLM__API_KEY");
    let dir = tempfile::tempdir().unwrap();
    let yaml = r#"
llm:
  api_mode: "responses"
  base_url: "https://custom.api/v1"
  api_key: "sk-full-key"
  model: "deepseek-v3"
  max_retries: 5

exploration:
  token_threshold: 15000
  token_target_ratio: 0.35
  refiner_summary_token_ratio: 0.08

deep_explorer:
  max_tool_calls: 50
  loop_warning_threshold: 5

conversation:
  round_threshold: 15
  token_threshold: 3000

workspace:
  path: "/home/user/my_project"
  init_script:
    enabled: true
    script_path: "./clone.sh"
    timeout_sec: 60

tools:
  shell_timeout_secs: 60
  shell_max_output_bytes: 20480

context:
  record_max_chars: 16000
  min_remaining_records: 10

logging:
  level: "debug"
"#;
    let path = write_config(&dir, yaml);
    let config = AppConfig::load(Some(&path)).expect("完整配置应加载成功");

    assert_eq!(config.llm.api_mode, "responses");
    assert_eq!(config.llm.base_url, "https://custom.api/v1");
    assert_eq!(config.llm.api_key, "sk-full-key");
    assert_eq!(config.llm.model, "deepseek-v3");
    assert_eq!(config.llm.max_retries, 5);
    assert_eq!(config.exploration.token_threshold, 15000);
    assert_eq!(config.deep_explorer.max_tool_calls, 50);
    assert_eq!(config.deep_explorer.loop_warning_threshold, 5);
    assert_eq!(config.tools.shell_timeout_secs, 60);
    assert_eq!(config.tools.shell_max_output_bytes, 20480);
    assert_eq!(config.context.record_max_chars, 16000);
    assert_eq!(config.context.min_remaining_records, 10);
    assert_eq!(config.workspace.path, "/home/user/my_project");
    assert_eq!(config.logging.level, "debug");
}

// CF-003: 无配置文件使用默认值
// 推导链：无配置文件 → 返回全默认 AppConfig
#[test]
fn cf_003_no_config_file_uses_defaults() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let saved = std::env::var("EXPLORE_CONFIG_PATH").ok();
    std::env::remove_var("EXPLORE_CONFIG_PATH");

    let config = AppConfig::load(None).expect("无配置文件应返回默认值");
    assert_eq!(config.llm.model, "deepseek-chat");
    assert_eq!(config.exploration.token_threshold, 12000);

    // Restore
    if let Some(v) = saved { std::env::set_var("EXPLORE_CONFIG_PATH", v); }
}

// CF-004: 环境变量覆盖 YAML 值
// 推导链：YAML model=gpt-4 + ENV EXPLORE_LLM__MODEL=deepseek-chat → model="deepseek-chat"
#[test]
fn cf_004_env_var_overrides_yaml() {
    let _guard = ENV_MUTEX.lock().unwrap();
    std::env::remove_var("EXPLORE_LLM__API_KEY");
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, "llm:\n  api_key: sk-from-yaml\n  model: gpt-4\n");

    std::env::set_var("EXPLORE_LLM__MODEL", "deepseek-chat-override");
    let config = AppConfig::load(Some(&path)).expect("配置应加载成功");
    assert_eq!(config.llm.model, "deepseek-chat-override");
    // api_key 来自 YAML 未被覆盖
    assert_eq!(config.llm.api_key, "sk-from-yaml");

    std::env::remove_var("EXPLORE_LLM__MODEL");
}

// CF-005: 环境变量补充缺失项
// 推导链：YAML 无 api_key + ENV EXPLORE_LLM__API_KEY=sk-env → api_key="sk-env"
#[test]
fn cf_005_env_var_supplements_missing() {
    let _guard = ENV_MUTEX.lock().unwrap();
    std::env::remove_var("EXPLORE_LLM__API_KEY");
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, "llm:\n  base_url: https://custom.api/v1\n");

    std::env::set_var("EXPLORE_LLM__API_KEY", "sk-from-env");
    let config = AppConfig::load(Some(&path)).expect("配置应加载成功");
    assert_eq!(config.llm.base_url, "https://custom.api/v1");
    assert_eq!(config.llm.api_key, "sk-from-env");

    std::env::remove_var("EXPLORE_LLM__API_KEY");
}

// CF-006: 配置文件不存在且无环境变量
// 推导链：指定路径不存在 → read_to_string 失败 → Err
#[test]
fn cf_006_missing_file_no_env_vars() {
    let config = AppConfig::load(Some("/nonexistent/path/config.yaml"));
    assert!(config.is_err(), "指定路径不存在应返回 Err");
    assert!(config.unwrap_err().contains("Failed to read"), "错误应含 'Failed to read'");
}

// CF-007: 非法 YAML 格式
// 推导链：语法错误 YAML → serde_yaml 解析失败 → Err
#[test]
fn cf_007_invalid_yaml_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, "llm:\n  api_key: [this, is, not, a, string\n");

    let config = AppConfig::load(Some(&path));
    assert!(config.is_err(), "非法 YAML 应返回 Err");
    let err = config.unwrap_err();
    assert!(err.contains("Failed to parse"), "错误信息应含 'Failed to parse'");
}

// ============================================================================
// 6.2 默认值测试 (CF-008 ~ CF-012)
// ============================================================================

// CF-008: 默认 token 阈值
#[test]
fn cf_008_default_token_threshold() {
    let json = r#"{"llm": {"api_key": "sk-test"}, "exploration": {}}"#;
    let config: AppConfig = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(config.exploration.token_threshold, 12000);
}

// CF-009: 默认 token 目标比例
#[test]
fn cf_009_default_token_target_ratio() {
    let json = r#"{"llm": {"api_key": "sk-test"}, "exploration": {}}"#;
    let config: AppConfig = serde_json::from_str(json).expect("反序列化失败");
    assert!((config.exploration.token_target_ratio - 0.40).abs() < f64::EPSILON);
}

// CF-010: 默认 LLM base_url
#[test]
fn cf_010_default_llm_base_url() {
    let json = r#"{"llm": {}}"#;
    let config: AppConfig = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(config.llm.base_url, "https://api.deepseek.com/v1");
}

// CF-011: 默认 API mode
#[test]
fn cf_011_default_api_mode() {
    let json = r#"{"llm": {}}"#;
    let config: AppConfig = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(config.llm.api_mode, "chat");
}

// CF-012: 默认 max_tool_calls
#[test]
fn cf_012_default_max_tool_calls() {
    let json = r#"{"llm": {}, "deep_explorer": {}}"#;
    let config: AppConfig = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(config.deep_explorer.max_tool_calls, 75);
}

// ============================================================================
// 补充：完整默认值验证
// ============================================================================

// CF-013: 全空 JSON 产生的完整默认值
#[test]
fn cf_013_all_defaults_from_empty_json() {
    let config: AppConfig = serde_json::from_str("{}").expect("反序列化失败");

    // LLM
    assert_eq!(config.llm.api_mode, "chat");
    assert_eq!(config.llm.base_url, "https://api.deepseek.com/v1");
    assert_eq!(config.llm.api_key, "");
    assert_eq!(config.llm.model, "deepseek-chat");
    assert_eq!(config.llm.max_retries, 3);

    // Exploration
    assert_eq!(config.exploration.token_threshold, 12000);
    assert!((config.exploration.token_target_ratio - 0.40).abs() < f64::EPSILON);
    // v1.2: max_fast_explore_rounds and early_termination_confidence removed

    // Deep Explorer
    assert_eq!(config.deep_explorer.max_tool_calls, 75);
    assert_eq!(config.deep_explorer.loop_warning_threshold, 3);

    // Conversation
    assert_eq!(config.conversation.round_threshold, 10);
    assert_eq!(config.conversation.token_threshold, 2000);

    // Tools
    assert_eq!(config.tools.shell_timeout_secs, 30);
    assert_eq!(config.tools.shell_max_output_bytes, 10240);

    // Context
    assert_eq!(config.context.record_max_chars, 8000);
    assert_eq!(config.context.min_remaining_records, 5);

    // Workspace
    assert_eq!(config.workspace.path, "./workspace");

    // Logging
    assert_eq!(config.logging.level, "info");
}

// CF-014: 部分字段覆盖后的混合配置
#[test]
fn cf_014_partial_override() {
    let json = r#"{
        "llm": {"api_key": "sk-custom"},
        "exploration": {"token_threshold": 8000},
        "deep_explorer": {"max_tool_calls": 30},
        "logging": {"level": "debug"}
    }"#;
    let config: AppConfig = serde_json::from_str(json).expect("反序列化失败");

    // 覆盖的值
    assert_eq!(config.llm.api_key, "sk-custom");
    assert_eq!(config.exploration.token_threshold, 8000);
    assert_eq!(config.deep_explorer.max_tool_calls, 30);
    assert_eq!(config.logging.level, "debug");

    // 其余仍是默认值
    assert_eq!(config.llm.model, "deepseek-chat");
    assert_eq!(config.tools.shell_timeout_secs, 30);
}

// ============================================================================
// v1.2: 配置清理测试 (CF-013 ~ CF-015)
// ============================================================================

// CF-013: 旧 SSA 配置项被忽略（v1.2 已从结构体中移除这些字段）
// 推导链：YAML 含已删除字段 → serde 默认忽略未知字段 → 加载成功不报错
#[test]
fn cf_013_legacy_ssa_config_ignored() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let yaml = r#"
llm:
  api_key: sk-test
exploration:
  token_threshold: 6000
  enable_fast_explore: false
"#;
    let path = write_config(&dir, yaml);
    let result = AppConfig::load(Some(&path));
    // v1.2: 旧字段不再存在于结构体中，serde 默认跳过未知字段，不报错
    assert!(result.is_ok(), "含已废弃字段的配置应正常加载，实际: {:?}", result.err());
}

// CF-014: 精简后的 exploration 配置加载
// 推导链：YAML 仅含 exploration.token_threshold 和 token_target_ratio → 加载成功
#[test]
fn cf_014_minimal_exploration_config_loads() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let yaml = r#"
llm:
  api_key: sk-test
exploration:
  token_threshold: 8000
  token_target_ratio: 0.50
"#;
    let path = write_config(&dir, yaml);
    let config = AppConfig::load(Some(&path)).expect("精简配置应加载成功");
    // token_threshold 和 token_target_ratio 从 YAML 读取
    assert_eq!(config.exploration.token_threshold, 8000);
    assert!((config.exploration.token_target_ratio - 0.50).abs() < 0.001);
}

// CF-015: exploration 段不存在
// 推导链：YAML 不含 exploration 段 → 所有 exploration 字段使用默认值 → 不 panic
#[test]
fn cf_015_no_exploration_section_uses_defaults() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, "llm:\n  api_key: sk-test\n");
    let config = AppConfig::load(Some(&path)).expect("无 exploration 段应正常加载");
    // 使用默认值
    assert!(config.exploration.token_threshold > 0, "token_threshold 应有默认值");
    assert!(config.exploration.token_target_ratio > 0.0, "token_target_ratio 应有默认值");
}

// CF-016: tools 段从 YAML 读取配置
#[test]
fn cf_016_tools_section_from_yaml() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let yaml = "\
llm:\n  api_key: sk-test\n\
tools:\n  shell_timeout_secs: 60\n  shell_max_output_bytes: 2048\n  shell_max_output_lines: 200\n";
    let path = write_config(&dir, yaml);
    let config = AppConfig::load(Some(&path)).expect("tools 段应正常加载");
    assert_eq!(config.tools.shell_timeout_secs, 60);
    assert_eq!(config.tools.shell_max_output_bytes, 2048);
    assert_eq!(config.tools.shell_max_output_lines, 200);
}

// CF-017: tools 段不存在使用默认值
#[test]
fn cf_017_no_tools_section_uses_defaults() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, "llm:\n  api_key: sk-test\n");
    let config = AppConfig::load(Some(&path)).expect("无 tools 段应正常加载");
    assert!(config.tools.shell_timeout_secs > 0, "shell_timeout_secs 应有默认值");
    assert!(config.tools.shell_max_output_bytes > 0, "shell_max_output_bytes 应有默认值");
    assert!(config.tools.shell_max_output_lines > 0, "shell_max_output_lines 应有默认值");
}

// CF-018: llm 段 context_limit / max_output_tokens 从 YAML 读取
#[test]
fn cf_018_llm_context_limits_from_yaml() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let yaml = "llm:\n  api_key: sk-test\n  context_limit: 65536\n  max_output_tokens: 4096\n";
    let path = write_config(&dir, yaml);
    let config = AppConfig::load(Some(&path)).expect("llm 段应正常加载");
    assert_eq!(config.llm.context_limit, Some(65536));
    assert_eq!(config.llm.max_output_tokens, Some(4096));
}

// CF-019: llm 段未设 context_limit / max_output_tokens 时为 None
#[test]
fn cf_019_llm_context_limits_default_none() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, "llm:\n  api_key: sk-test\n");
    let config = AppConfig::load(Some(&path)).expect("llm 段应正常加载");
    assert_eq!(config.llm.context_limit, None, "未设置时应为 None");
    assert_eq!(config.llm.max_output_tokens, None, "未设置时应为 None");
}
