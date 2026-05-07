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

/// ES-043: Output truncation (> 10 KB)
#[test]
fn es_043_output_truncation() {
    let fixture = common::create_test_fixture();
    let root = fixture.path();
    let tool = make_tool(root);

    // Create a ~20 KB file (just enough to exceed 10 KB truncation limit)
    // instead of using the 10 MB large_file.txt which times out on Windows pipes.
    let line = "repeated line for truncation test abcdefghij\n";
    let content: String = line.repeat((12 * 1024 / line.len()) + 50);
    std::fs::write(root.join("trunc_test.txt"), &content).unwrap();

    let input = make_input(root, serde_json::json!({
        "command": "cat trunc_test.txt"
    }));

    let result = tool.execute(input).expect("should succeed");
    let output: ExecuteShellOutput = serde_json::from_value(result.data).unwrap();

    // Output should be truncated to ~10 KB
    assert!(output.output.len() <= 10 * 1024 + 512); // small tolerance
    assert!(result.truncated);
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
