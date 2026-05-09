mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::models::ToolInput;
use explore_ai_agent::tools::executor::ToolExecutor;
use explore_ai_agent::tools::execute_shell::{ExecuteShellTool, ExecuteShellOutput, ShellSecurity};

fn make_tool(root: &std::path::Path) -> ExecuteShellTool {
    ExecuteShellTool::new(root.to_path_buf())
}

fn make_input(root: &std::path::Path, params: serde_json::Value) -> ToolInput {
    ToolInput {
        tool_name: "execute_shell".to_string(),
        params,
        project_root: root.to_path_buf(),
    }
}

// ===== 8.7.1 Whitelist Tests =====

/// ES-001: Whitelisted command - grep
#[test]
fn es_001_whitelist_grep() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "grep -rn main src/"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.output.contains("main"));
}

/// ES-002: Whitelisted command - cat
#[test]
fn es_002_whitelist_cat() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "cat src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.output.contains("fn main"));
}

/// ES-003: Whitelisted command - find
#[test]
fn es_003_whitelist_find() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "find src -name '*.rs'"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.output.contains("main.rs"));
}

/// ES-004: Whitelisted command - wc
#[test]
fn es_004_whitelist_wc() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "wc -l src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-005: Whitelisted command - ls
#[test]
fn es_005_whitelist_ls() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls src"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-006: Whitelisted command - head
#[test]
fn es_006_whitelist_head() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "head -5 src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-007: Whitelisted command - tail
#[test]
fn es_007_whitelist_tail() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "tail -5 src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-008: Whitelisted command - sort
#[test]
fn es_008_whitelist_sort() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "sort src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-009: Whitelisted command - awk
#[test]
fn es_009_whitelist_awk() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "awk '{print NR}' src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-010: Whitelisted command - sed (read-only)
#[test]
fn es_010_whitelist_sed_readonly() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "sed -n '1,5p' src/main.rs"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-011: Non-whitelisted - rm
#[test]
fn es_011_blocked_rm() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "rm src/main.rs"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellCmdNotAllowed);
}

/// ES-012: Non-whitelisted - mv
#[test]
fn es_012_blocked_mv() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "mv src/main.rs src/old.rs"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellCmdNotAllowed);
}

/// ES-013: Non-whitelisted - curl
#[test]
fn es_013_blocked_curl() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "curl https://example.com"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellCmdNotAllowed);
}

/// ES-014: Non-whitelisted - python
#[test]
fn es_014_blocked_python() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "python -c 'print(1)'"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellCmdNotAllowed);
}

/// ES-015: sed -i blocked
#[test]
fn es_015_sed_inplace_blocked() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "sed -i 's/old/new/' src/main.rs"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellCmdNotAllowed);
}

// ===== 8.7.2 Dangerous Operator Tests =====

/// ES-020: Output redirect >
#[test]
fn es_020_redirect_out() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls > output.txt"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-021: Append redirect >>
#[test]
fn es_021_redirect_append() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls >> output.txt"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-022: Command substitution $()
#[test]
fn es_022_command_substitution() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "echo $(rm -rf /)"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-023: Backtick command substitution
#[test]
fn es_023_backtick_substitution() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "echo `whoami`"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-024: Semicolon separator
#[test]
fn es_024_semicolon() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls; rm -rf /"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-025: Logical AND &&
#[test]
fn es_025_logical_and() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls && rm -rf /"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-026: Logical OR ||
#[test]
fn es_026_logical_or() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls || rm -rf /"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-028: tee command
#[test]
fn es_028_tee() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls | tee output.txt"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-029: awk system() call
#[test]
fn es_029_awk_system() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "awk '{system(\"rm -rf /\")}' file"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellDangerousOperator);
}

/// ES-030: Legal pipe (whitelist | whitelist)
#[test]
fn es_030_legal_pipe() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "grep main src/main.rs | wc -l"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-031: Pipe to non-whitelisted command
#[test]
fn es_031_pipe_non_whitelist() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls | python -c 'import sys'"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::ShellCmdNotAllowed);
}

// ===== 8.7.3 Execution Environment Tests =====

/// ES-040: Specify working_dir
#[test]
fn es_040_working_dir() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls",
        "working_dir": "src"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.output.contains("main.rs"));
}

/// ES-041: working_dir path traversal
#[test]
fn es_041_working_dir_traversal() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls",
        "working_dir": "../../"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathOutsideRoot);
}

/// ES-042: working_dir not found
#[test]
fn es_042_working_dir_not_found() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls",
        "working_dir": "nonexistent"
    }));

    let result = tool.execute(input);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathNotFound);
}

/// ES-043: Output truncation (> 50 KB)
#[test]
fn es_043_output_truncation() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // Create a ~55 KB file to exceed the 50 KB truncation limit
    let line = "repeated line for truncation test abcdefghij\n";
    let content: String = line.repeat((55 * 1024 / line.len()) + 50);
    std::fs::write(root.join("trunc_test.txt"), &content).unwrap();

    let cat_cmd = if cfg!(target_os = "windows") {
        format!("type {}", root.join("trunc_test.txt").display())
    } else {
        format!("cat {}", root.join("trunc_test.txt").display())
    };
    let input = make_input(root, serde_json::json!({"command": cat_cmd}));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.output.len() <= 51 * 1024, "must be truncated at 50KB threshold");
    assert!(result.truncated, "truncated flag must be set");
}

/// ES-044: Command failure (non-zero exit code)
#[test]
fn es_044_command_failure() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "grep zzz_definitely_not_found src/main.rs"
    }));

    let result = tool.execute(input).expect("should return result, not tool error");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    // grep returns exit code 1 when no match
    assert!(!output.success);
}

/// ES-045: Default working_dir
#[test]
fn es_045_default_working_dir() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "ls"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.output.contains("src"));
}

// ===== 8.7.4 exclude_paths Injection Tests =====

/// ES-050: grep exclude single path
#[test]
fn es_050_grep_exclude_single() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // "fn " exists in Rust files under src/, so grep will find matches and succeed
    let input = make_input(root, serde_json::json!({
        "command": "grep -rn \"fn \" src/",
        "exclude_paths": ["vendor/*"]
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success, "grep should find matches for 'fn ' in src/");
}

/// ES-051: grep exclude multiple paths
#[test]
fn es_051_grep_exclude_multiple() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "grep -rn TODO .",
        "exclude_paths": ["vendor/*", "opensource/*"]
    }));

    let result = tool.execute(input);
    // Should succeed or fail gracefully (no matches), but command should be valid
    assert!(result.is_ok() || {
        let err = result.unwrap_err();
        err.code != ErrorCode::ShellDangerousOperator && err.code != ErrorCode::ShellCmdNotAllowed
    });
}

/// ES-052: find exclude paths
#[test]
fn es_052_find_exclude() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "find . -name '*.java'",
        "exclude_paths": ["test/*"]
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    // Output should not contain files from test/ directory
    assert!(!output.output.contains("test/"));
}

/// ES-053: Non-traversal command ignores exclude_paths
#[test]
fn es_053_non_traversal_ignores_exclude() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "cat src/main.rs",
        "exclude_paths": ["vendor/*"]
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
    assert!(output.output.contains("fn main"));
}

/// ES-054: Empty exclude_paths
#[test]
fn es_054_empty_exclude() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "grep -rn main src/",
        "exclude_paths": []
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(output.success);
}

/// ES-055: Injected exclude_paths pass safety check
#[test]
fn es_055_injected_paths_pass_safety() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "grep -rn main src/",
        "exclude_paths": ["vendor/*"]
    }));

    let result = tool.execute(input);
    // Must not fail with SHELL_DANGEROUS_OPERATOR
    assert!(result.is_ok() || result.as_ref().unwrap_err().code != ErrorCode::ShellDangerousOperator);
}

// ===== Unit tests for ShellSecurity =====

#[test]
fn whitelist_allows_valid_commands() {
    for cmd in ShellSecurity::WHITELIST {
        assert!(
            ShellSecurity::check_whitelist(cmd).is_ok(),
            "Command '{}' should be whitelisted",
            cmd
        );
    }
}

#[test]
fn whitelist_blocks_dangerous_commands() {
    let blocked = vec!["rm", "mv", "cp", "chmod", "chown", "curl", "wget", "python", "node", "bash", "sh"];
    for cmd in blocked {
        assert!(
            ShellSecurity::check_whitelist(cmd).is_err(),
            "Command '{}' should be blocked",
            cmd
        );
    }
}

#[test]
fn dangerous_operators_detected() {
    let dangerous = vec![
        "ls > out.txt",
        "ls >> out.txt",
        "echo $(whoami)",
        "echo `whoami`",
        "ls; rm -rf /",
        "ls && rm -rf /",
        "ls || rm -rf /",
        "sleep 100 &",
        "awk '{system(\"ls\")}' f",
        "awk '{exec(\"ls\")}' f",
    ];
    for cmd in dangerous {
        assert!(
            ShellSecurity::check_dangerous_operators(cmd).is_err(),
            "Command '{}' should be flagged as dangerous",
            cmd
        );
    }
}

#[test]
fn safe_operators_allowed() {
    let safe = vec![
        "grep main file.rs",
        "cat src/main.rs",
        "find . -name '*.rs'",
        "wc -l file.txt",
    ];
    for cmd in safe {
        assert!(
            ShellSecurity::check_dangerous_operators(cmd).is_ok(),
            "Command '{}' should be considered safe",
            cmd
        );
    }
}

#[test]
fn sed_inplace_detected() {
    assert!(ShellSecurity::check_sed_inplace("sed -i 's/a/b/' file").is_err());
    assert!(ShellSecurity::check_sed_inplace("sed -n '1,5p' file").is_ok());
}

#[test]
fn pipe_all_segments_must_be_whitelisted() {
    assert!(ShellSecurity::check_pipe_commands("grep main | wc -l").is_ok());
    assert!(ShellSecurity::check_pipe_commands("grep main | sort | uniq").is_ok());
    assert!(ShellSecurity::check_pipe_commands("grep main | python").is_err());
    assert!(ShellSecurity::check_pipe_commands("ls | tee out.txt").is_err());
}

#[test]
fn validate_command_full_pipeline() {
    assert!(ShellSecurity::validate_command("grep -rn main src/").is_ok());
    assert!(ShellSecurity::validate_command("rm -rf /").is_err());
    assert!(ShellSecurity::validate_command("ls > out.txt").is_err());
    assert!(ShellSecurity::validate_command("grep main | wc -l").is_ok());
    assert!(ShellSecurity::validate_command("grep main | python").is_err());
    assert!(ShellSecurity::validate_command("sed -i 's/a/b/' f").is_err());
}

#[test]
fn inject_exclude_paths_grep() {
    let result = ShellSecurity::inject_exclude_paths(
        "grep -rn pattern src/",
        &["vendor/*".to_string(), "opensource/*".to_string()],
    );
    assert!(result.contains("--exclude-dir=vendor"));
    assert!(result.contains("--exclude-dir=opensource"));
}

#[test]
fn inject_exclude_paths_find() {
    let result = ShellSecurity::inject_exclude_paths(
        "find . -name '*.java'",
        &["test/*".to_string()],
    );
    assert!(result.contains("-not -path"));
}

#[test]
fn inject_exclude_paths_other_commands_ignored() {
    let original = "cat src/main.rs";
    let result = ShellSecurity::inject_exclude_paths(
        original,
        &["vendor/*".to_string()],
    );
    assert_eq!(result, original);
}

// ============================================================================
// v1.2: 端到端真实 Shell 测试
// ============================================================================

/// E2E-001: cat/type real file + verify content
#[test]
fn e2e_001_read_file_via_shell() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let content = "hello world\nfoo bar\nbaz qux\n";
    std::fs::write(root.join("test.txt"), content).unwrap();

    let cmd = if cfg!(target_os = "windows") {
        format!("type {}", root.join("test.txt").display())
    } else {
        format!("cat {}", root.join("test.txt").display())
    };
    let input = make_input(root, serde_json::json!({"command": cmd}));
    let result = tool.execute(input).expect("cat must succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();
    assert!(output.success, "cat should succeed");
    assert!(output.output.contains("hello world"),
        "output must contain file content, got: {}", output.output);
    assert!(output.output.contains("foo bar"), "output must contain all lines");
}

/// E2E-002: grep text search with real file
#[test]
fn e2e_002_grep_real_file() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    std::fs::write(root.join("code.rs"), "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    let cmd = if cfg!(target_os = "windows") {
        format!("findstr main {}", root.join("code.rs").display())
    } else {
        format!("grep main {}", root.join("code.rs").display())
    };
    let input = make_input(root, serde_json::json!({"command": cmd}));
    let result = tool.execute(input).expect("grep must succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();
    assert!(output.success, "grep should succeed: {:?}", output.error);
    assert!(output.output.contains("fn main"), "grep must find 'fn main' line");
}

/// E2E-003: echo piped to grep (safe pipeline)
#[test]
fn e2e_003_echo_pipe_grep() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({"command": "echo hello | grep hello"}));
    let result = tool.execute(input).expect("pipe must succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();
    assert!(output.success, "pipe should succeed: {:?}", output.error);
}

/// E2E-004: disallowed command returns clear error
#[test]
fn e2e_004_disallowed_command_error() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({"command": "rm -rf /"}));
    let result = tool.execute(input);
    // rm should be rejected by whitelist or operator check
    assert!(result.is_err(), "rm must be rejected");
    let err = result.unwrap_err();
    assert!(
        err.error.contains("not in") || err.error.contains("not allowed") || err.error.contains("拒绝") || err.error.contains("whitelist"),
        "error must explain rejection, got: error={}", err.error
    );
}

/// E2E-005: Shell discovery returns usable shell
#[test]
fn e2e_005_shell_discovery_returns_valid() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // Execute a simple command to verify the discovered shell works
    let input = make_input(root, serde_json::json!({"command": "echo shell_works"}));
    let result = tool.execute(input).expect("shell must be functional");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();
    assert!(output.success, "discovered shell must be able to execute echo");
    assert!(output.output.contains("shell_works"),
        "shell output must contain the echoed text, got: {}", output.output);
}

/// E2E-006: > inside single quotes (awk) must NOT trigger redirect
#[test]
fn e2e_006_awk_comparison_in_quotes() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // awk '{ if (NF>1) print $NF }' — > inside '...' should be safe
    let input = make_input(root, serde_json::json!({
        "command": "echo a.txt | awk -F. '{ if (NF>1) print $NF }'"
    }));
    let result = tool.execute(input);
    assert!(result.is_ok(),
        "> inside single quotes must NOT trigger redirect, got: {:?}", result.err());
}

/// E2E-007: ; inside single quotes must NOT trigger separator
#[test]
fn e2e_007_semicolon_in_quotes() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // awk uses ; as statement separator inside quotes
    let input = make_input(root, serde_json::json!({
        "command": "echo test | awk '{ i=1; print $i }'"
    }));
    let result = tool.execute(input);
    assert!(result.is_ok(),
        "; inside single quotes must NOT trigger separator, got: {:?}", result.err());
}

/// E2E-008: > inside double quotes must NOT trigger redirect
#[test]
fn e2e_008_redirect_in_double_quotes() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "echo \"a > b\" | grep a"
    }));
    let result = tool.execute(input);
    assert!(result.is_ok(),
        "> inside double quotes must NOT trigger redirect, got: {:?}", result.err());
}

/// E2E-009: real ; outside quotes must still be rejected
#[test]
fn e2e_009_real_semicolon_rejected() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let input = make_input(root, serde_json::json!({
        "command": "echo hello; echo world"
    }));
    let result = tool.execute(input);
    assert!(result.is_err(),
        "real ; outside quotes must still be rejected");
}

/// E2E-010: file extension counting pipeline (the user's real scenario)
#[test]
fn e2e_010_file_extension_counting() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // Create files with different extensions
    std::fs::write(root.join("main.rs"), "// rust").unwrap();
    std::fs::write(root.join("lib.rs"), "// rust lib").unwrap();
    std::fs::write(root.join("config.toml"), "# toml").unwrap();
    std::fs::write(root.join("README.md"), "# readme").unwrap();
    std::fs::write(root.join("test.py"), "# python").unwrap();
    std::fs::write(root.join("helper.py"), "# python helper").unwrap();

    // Count file extensions (skip files without extension)
    let input = make_input(root, serde_json::json!({
        "command": "ls | grep '\\.' | awk -F. '{print $NF}' | sort | uniq -c"
    }));
    let result = tool.execute(input);
    assert!(result.is_ok(),
        "file extension counting must succeed, got: {:?}", result.err());
    let output: ExecuteShellOutput = serde_json::from_value(result.unwrap().data).unwrap();
    assert!(output.success, "pipeline must succeed: {:?}", output.error);
    assert!(output.output.contains("md"), "must find .md files: {}", output.output);
    assert!(output.output.contains("rs"), "must find .rs files: {}", output.output);
    assert!(output.output.contains("py"), "must find .py files: {}", output.output);
}
// ============================================================================

/// ES-062: 50KB 截断（C 层字节截断）
/// Note: 当前 MAX_SHELL_OUTPUT_BYTES=10KB，升到 50KB 后此测试生效
#[test]
fn es_062_output_truncation_50kb() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // Create a ~60 KB file to exceed the 50 KB C-layer limit
    let line = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
    let content: String = line.repeat((55 * 1024 / line.len()) + 50);
    std::fs::write(root.join("large_50kb.txt"), &content).unwrap();

    let cat_cmd = if cfg!(target_os = "windows") {
        format!("type {}", root.join("large_50kb.txt").display())
    } else {
        format!("cat {}", root.join("large_50kb.txt").display())
    };
    let input = make_input(root, serde_json::json!({"command": cat_cmd}));
    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(result.truncated, "must be truncated at 50KB threshold");
    // Allow small tolerance above 50KB
    assert!(output.output.len() <= 51 * 1024,
        "output must be ≤~51KB, got {}", output.output.len());
}

/// ES-063: 小输出不截断
#[test]
fn es_063_small_output_not_truncated() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    let echo_cmd = if cfg!(target_os = "windows") {
        "echo hello".to_string()
    } else {
        "echo hello".to_string()
    };
    let input = make_input(root, serde_json::json!({"command": echo_cmd}));
    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(!result.truncated, "small output must not be truncated");
    assert!(output.output.contains("hello"), "output must contain echo text");
}

/// ES-064: 截断边界——输出恰好小于 50KB 不触发截断
/// Note: 当前 MAX_SHELL_OUTPUT_BYTES=10KB，升到 50KB 后此测试生效
#[test]
fn es_064_boundary_below_threshold() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // Create a file just under 50 KB
    let line = "x".repeat(99) + "\n"; // 100 chars per line
    let content: String = line.repeat(49 * 1024 / 100);
    std::fs::write(root.join("boundary.txt"), &content).unwrap();

    let cat_cmd = if cfg!(target_os = "windows") {
        format!("type {}", root.join("boundary.txt").display())
    } else {
        format!("cat {}", root.join("boundary.txt").display())
    };
    let input = make_input(root, serde_json::json!({"command": cat_cmd}));
    let result = tool.execute(input).expect("should succeed");

    assert!(!result.truncated,
        "output under threshold must not be truncated");
}

/// ES-065: 多字节字符处理
#[test]
fn es_065_multibyte_characters() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // 1000 Chinese characters (≈3000 bytes), far below any threshold
    let content: String = "中".repeat(1000);
    std::fs::write(root.join("chinese.txt"), &content).unwrap();

    let cat_cmd = if cfg!(target_os = "windows") {
        format!("type {}", root.join("chinese.txt").display())
    } else {
        format!("cat {}", root.join("chinese.txt").display())
    };
    let input = make_input(root, serde_json::json!({"command": cat_cmd}));
    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    assert!(!result.truncated,
        "output far below threshold must not be truncated");
    assert!(output.output.contains('中'),
        "output must contain Chinese characters");
}

/// ES-066: 2000 行截断（C 层不截断，Rust 层截断至 2000 行）
#[test]
fn es_066_line_truncation_rust_layer() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tool = make_tool(tmp.path());

    // Build 2500 lines (each short, total < 50KB)
    let mut content = String::new();
    for i in 0..2500 {
        content.push_str(&format!("line {}\n", i));
    }
    let file_path = tmp.path().join("many_lines.txt");
    std::fs::write(&file_path, &content).unwrap();

    let cat_cmd = if cfg!(target_os = "windows") {
        format!("type {}", file_path.display())
    } else {
        format!("cat {}", file_path.display())
    };

    let input = ToolInput {
        tool_name: "execute_shell".to_string(),
        params: serde_json::json!({"command": cat_cmd}),
        project_root: tmp.path().to_path_buf(),
    };
    let result = tool.execute(input).unwrap();
    assert!(result.truncated, "2500 lines must trigger truncation");
    assert!(result.success, "execution must succeed");

    // Verify output is capped at 2000 lines
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();
    let lines: Vec<&str> = output.output.lines().collect();
    assert!(lines.len() <= 2000,
        "output must be truncated to ≤2000 lines, got {}", lines.len());
}
