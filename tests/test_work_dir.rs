use explore_ai_agent::common::work_dir::*;
use std::fs;

// ============================================================================
// Helpers
// ============================================================================

fn make_config(path: &str, init_script: Option<InitScriptConfig>) -> WorkspaceConfig {
    WorkspaceConfig {
        path: path.to_string(),
        init_script,
    }
}

fn make_temp_dir() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("创建临时目录失败");
    let path = dir.path().to_string_lossy().to_string();
    (dir, path)
}

// ============================================================================
// 7.2 数据结构和构造测试 (WI-001 ~ WI-003)
// ============================================================================

// WI-001: WorkspaceConfig 反序列化（完整配置）
#[test]
fn wi_001_workspace_config_full_deserialize() {
    let json = r#"{
        "path": "./workspace",
        "init_script": {
            "enabled": true,
            "script_path": "./scripts/init.sh",
            "timeout_sec": 60
        }
    }"#;
    let config: WorkspaceConfig = serde_json::from_str(json).expect("反序列化失败");

    assert_eq!(config.path, "./workspace");
    let script = config.init_script.expect("应含 init_script");
    assert!(script.enabled);
    assert_eq!(script.script_path, "./scripts/init.sh");
    assert_eq!(script.timeout_sec, 60);
}

// WI-002: WorkspaceConfig 反序列化（最小配置）
#[test]
fn wi_002_workspace_config_minimal_deserialize() {
    let json = r#"{"path": "./workspace"}"#;
    let config: WorkspaceConfig = serde_json::from_str(json).expect("反序列化失败");

    assert_eq!(config.path, "./workspace");
    assert!(config.init_script.is_none());
}

// WI-003: 构造 WorkDirInitializer
#[test]
fn wi_003_constructor_does_not_panic() {
    let config = make_config("./workspace", None);
    let wi = WorkDirInitializer::new(config);
    let _ = wi;
}

// ============================================================================
// 7.3 check_workspace 测试 (WI-004 ~ WI-009)
// ============================================================================

// WI-004: 工作目录就绪（含文件）
// 推导链：目录含 main.rs → read_dir 返回 1 条 → Ready { item_count: 1 }
#[test]
fn wi_004_workspace_ready_with_files() {
    let (_dir, path) = make_temp_dir();
    fs::write(format!("{}/main.rs", path), "fn main() {}").unwrap();

    let config = make_config(&path, None);
    let wi = WorkDirInitializer::new(config);
    let result = wi.check_workspace().expect("应返回 Ok");
    assert_eq!(result, WorkspaceStatus::Ready { item_count: 1 });
}

// WI-005: 工作目录就绪（含子目录）
// 推导链：目录含 src/ → read_dir 返回 1 条 → Ready { item_count: 1 }
#[test]
fn wi_005_workspace_ready_with_subdir() {
    let (_dir, path) = make_temp_dir();
    fs::create_dir(format!("{}/src", path)).unwrap();

    let config = make_config(&path, None);
    let wi = WorkDirInitializer::new(config);
    let result = wi.check_workspace().expect("应返回 Ok");
    assert_eq!(result, WorkspaceStatus::Ready { item_count: 1 });
}

// WI-006: 工作目录就绪（含隐藏文件）
// 推导链：目录含 .gitignore → read_dir 返回 1 条 → Ready { item_count: 1 }
#[test]
fn wi_006_workspace_ready_with_hidden_file() {
    let (_dir, path) = make_temp_dir();
    fs::write(format!("{}/.gitignore", path), "target/").unwrap();

    let config = make_config(&path, None);
    let wi = WorkDirInitializer::new(config);
    let result = wi.check_workspace().expect("应返回 Ok");
    assert_eq!(result, WorkspaceStatus::Ready { item_count: 1 });
}

// WI-007: 工作目录为空（无脚本）
// 推导链：空目录 + init_script = None → EmptyWithoutScript
#[test]
fn wi_007_workspace_empty_without_script() {
    let (_dir, path) = make_temp_dir();

    let config = make_config(&path, None);
    let wi = WorkDirInitializer::new(config);
    let result = wi.check_workspace().expect("应返回 Ok");
    assert_eq!(result, WorkspaceStatus::EmptyWithoutScript);
}

// WI-008: 工作目录为空（有脚本）
// 推导链：空目录 + init_script.enabled = true → EmptyWithScript
#[test]
fn wi_008_workspace_empty_with_script() {
    let (_dir, path) = make_temp_dir();

    let config = make_config(
        &path,
        Some(InitScriptConfig {
            enabled: true,
            script_path: "./init.sh".to_string(),
            timeout_sec: 120,
        }),
    );
    let wi = WorkDirInitializer::new(config);
    let result = wi.check_workspace().expect("应返回 Ok");
    assert_eq!(result, WorkspaceStatus::EmptyWithScript);
}

// WI-009: 工作目录路径不存在
// 推导链：path 不存在 → EmptyWithoutScript
#[test]
fn wi_009_workspace_path_not_found() {
    let config = make_config("/tmp/nonexistent_workspace_xyz", None);
    let wi = WorkDirInitializer::new(config);
    let result = wi.check_workspace().expect("应返回 Ok");
    assert_eq!(result, WorkspaceStatus::EmptyWithoutScript);
}

// ============================================================================
// 7.4 run_init_script 测试 (WI-010 ~ WI-013)
// ============================================================================

// WI-010: 脚本未启用
// 推导链：enabled = false → run_init_script 直接返回 Ok(())
#[test]
fn wi_010_script_not_enabled() {
    let config = make_config(
        "./workspace",
        Some(InitScriptConfig {
            enabled: false,
            script_path: "./init.sh".to_string(),
            timeout_sec: 120,
        }),
    );
    let wi = WorkDirInitializer::new(config);
    let result = wi.run_init_script("/tmp/workspace");
    assert!(result.is_ok(), "脚本未启用应返回 Ok");
}

// WI-011: 无脚本配置
// 推导链：init_script = None → run_init_script 直接返回 Ok(())
#[test]
fn wi_011_no_script_config() {
    let config = make_config("./workspace", None);
    let wi = WorkDirInitializer::new(config);
    let result = wi.run_init_script("/tmp/workspace");
    assert!(result.is_ok(), "无脚本配置应返回 Ok");
}

// WI-012: 脚本执行成功 (Unix only — uses /bin/sh)
#[test]
#[cfg(not(target_os = "windows"))]
fn wi_012_script_execution_success() {
    let (_dir, path) = make_temp_dir();
    let script_path = format!("{}/success.sh", path);
    fs::write(&script_path, "echo done\nexit 0\n").unwrap();

    let config = make_config(
        &path,
        Some(InitScriptConfig {
            enabled: true,
            script_path,
            timeout_sec: 120,
        }),
    );
    let wi = WorkDirInitializer::new(config);
    let result = wi.run_init_script(&path);
    assert!(result.is_ok(), "脚本执行成功应返回 Ok");
}

// WI-013: 脚本执行失败 (Unix only)
// 推导链：脚本 exit 1 → InitError::ScriptFailed { exit_code: Some(1), ... }
#[test]
#[cfg(not(target_os = "windows"))]
fn wi_013_script_execution_failure() {
    let (_dir, path) = make_temp_dir();
    let script_path = format!("{}/fail.sh", path);
    fs::write(&script_path, "echo failed\nexit 1\n").unwrap();

    let config = make_config(
        &path,
        Some(InitScriptConfig {
            enabled: true,
            script_path,
            timeout_sec: 120,
        }),
    );
    let wi = WorkDirInitializer::new(config);
    let result = wi.run_init_script(&path);
    assert!(result.is_err(), "脚本执行失败应返回 Err");
    match result.unwrap_err() {
        InitError::ScriptFailed { exit_code, .. } => {
            assert_eq!(exit_code, Some(1));
        }
        InitError::Timeout => panic!("应返回 ScriptFailed 而非 Timeout"),
    }
}
