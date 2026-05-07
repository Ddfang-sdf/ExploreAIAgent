use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::common::path_manager::PathManager;
use crate::common::config::ToolsConfig;
use crate::common::truncation::{MAX_SHELL_OUTPUT_BYTES, SHELL_TIMEOUT_SECS};
use crate::ffi_bridge::{ShellExecutorFFI};
use super::executor::ToolExecutor;

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteShellParams {
    pub command: String,
    #[serde(default = "default_path")]
    pub working_dir: String,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
}

fn default_path() -> String {
    ".".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteShellOutput {
    pub success: bool,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct ShellSecurity;

impl ShellSecurity {
    pub const WHITELIST: &'static [&'static str] = &[
        "cat", "head", "tail", "less",
        "grep", "egrep", "fgrep", "find",
        "ls", "tree",
        "wc", "sort", "uniq", "cut", "tr",
        "awk", "sed",
        "file", "stat",
        "echo",
        // Windows equivalents
        "type", "dir", "findstr",
    ];

    pub fn check_whitelist(command: &str) -> Result<(), String> {
        let trimmed = command.trim();
        let main_cmd = trimmed
            .split(|c: char| c.is_whitespace() || c == ';' || c == '|' || c == '&')
            .next()
            .unwrap_or("");

        if Self::WHITELIST.contains(&main_cmd) {
            if main_cmd == "sed" {
                Self::check_sed_inplace(trimmed)?;
            }
            Ok(())
        } else {
            Err(format!("Command '{}' is not in the whitelist", main_cmd))
        }
    }

    pub fn check_dangerous_operators(command: &str) -> Result<(), String> {
        // --- output redirect ---
        if command.contains(">>") {
            return Err("Append redirect >> detected".to_string());
        }

        // tee can write to files (same category as redirect)
        {
            let tokens: Vec<&str> = command.split_whitespace().collect();
            for token in &tokens {
                if *token == "tee" {
                    return Err("tee command detected (dangerous: can write files)".to_string());
                }
            }
        }

        // --- command substitution ---
        if command.contains("$(") {
            return Err("Command substitution $() detected".to_string());
        }
        if command.contains('`') {
            return Err("Backtick command substitution detected".to_string());
        }

        // --- command separators ---
        if command.contains(';') {
            return Err("Command separator ; detected".to_string());
        }
        if command.contains("&&") {
            return Err("Logical AND && detected".to_string());
        }
        if command.contains("||") {
            return Err("Logical OR || detected".to_string());
        }

        // --- system calls in awk etc. ---
        if command.contains("system(") || command.contains("exec(") {
            return Err("System/exec call detected in command".to_string());
        }

        // --- path traversal ---
        if command.contains("../") || command.contains("..\\") {
            return Err("Path traversal ../ detected".to_string());
        }

        // --- single > (not >>) ---
        let has_redirect = {
            let chars: Vec<char> = command.chars().collect();
            let mut found = false;
            for i in 0..chars.len() {
                if chars[i] == '>' {
                    if i + 1 < chars.len() && chars[i + 1] == '>' {
                        continue;
                    }
                    if i > 0 && chars[i - 1] == '>' {
                        continue;
                    }
                    found = true;
                    break;
                }
            }
            found
        };
        if has_redirect {
            return Err("Output redirect > detected".to_string());
        }

        // --- background & (not &&) ---
        let has_background = {
            let trimmed = command.trim();
            if trimmed.ends_with('&') {
                let prefix = trimmed[..trimmed.len() - 1].trim_end();
                !prefix.ends_with('&')
            } else {
                let re = regex::Regex::new(r"(?:^|[^&])&(?:[^&]|$)").unwrap();
                let without_double = command.replace("&&", "");
                re.is_match(&without_double)
            }
        };
        if has_background {
            return Err("Background execution & detected".to_string());
        }

        Ok(())
    }

    pub fn check_sed_inplace(command: &str) -> Result<(), String> {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        for token in &tokens[1..] {
            if *token == "-i" || token.starts_with("-i") {
                return Err("sed -i (in-place edit) is not allowed".to_string());
            }
        }
        Ok(())
    }

    pub fn check_pipe_commands(command: &str) -> Result<(), String> {
        if !command.contains('|') {
            return Ok(());
        }

        let segments: Vec<&str> = command.split('|').collect();
        for segment in &segments {
            let trimmed = segment.trim();
            let main_cmd = trimmed.split_whitespace().next().unwrap_or("");

            if main_cmd == "tee" {
                return Err("tee command detected (dangerous: can write files)".to_string());
            }

            if !Self::WHITELIST.contains(&main_cmd) {
                return Err(format!("Pipe segment command '{}' is not in the whitelist", main_cmd));
            }

            if main_cmd == "sed" {
                Self::check_sed_inplace(trimmed)?;
            }

            Self::check_dangerous_operators(trimmed)?;
        }

        Ok(())
    }

    pub fn validate_command(command: &str) -> Result<(), (ErrorCode, String)> {
        Self::check_whitelist(command)
            .map_err(|e| (ErrorCode::ShellCmdNotAllowed, e))?;

        Self::check_dangerous_operators(command)
            .map_err(|e| (ErrorCode::ShellDangerousOperator, e))?;

        Self::check_pipe_commands(command)
            .map_err(|e| {
                if e.contains("not in the whitelist") {
                    (ErrorCode::ShellCmdNotAllowed, e)
                } else {
                    // tee and other dangerous patterns → ShellDangerousOperator
                    (ErrorCode::ShellDangerousOperator, e)
                }
            })?;

        Ok(())
    }

    pub fn inject_exclude_paths(command: &str, exclude_paths: &[String]) -> String {
        if exclude_paths.is_empty() {
            return command.to_string();
        }

        let trimmed = command.trim();
        let main_cmd = trimmed.split_whitespace().next().unwrap_or("");

        match main_cmd {
            "grep" | "egrep" | "fgrep" => {
                let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
                if parts.len() < 2 {
                    return command.to_string();
                }
                let cmd_name = parts[0];
                let rest = parts[1];

                let excludes: Vec<String> = exclude_paths.iter().map(|p| {
                    let dir = p.trim_end_matches("/*").trim_end_matches("\\*");
                    format!("--exclude-dir={}", dir)
                }).collect();

                format!("{} {} {}", cmd_name, excludes.join(" "), rest)
            }
            "find" => {
                let mut result = trimmed.to_string();
                for path in exclude_paths {
                    let _clean = path.trim_end_matches("/*").trim_end_matches("\\*");
                    result = format!("{} -not -path './{}'", result, path);
                }
                result
            }
            _ => {
                command.to_string()
            }
        }
    }
}

pub struct ExecuteShellTool {
    path_manager: PathManager,
    shell_timeout_secs: u32,
    shell_max_output_bytes: usize,
}

impl ExecuteShellTool {
    pub fn new(project_root: PathBuf) -> Self {
        ExecuteShellTool {
            path_manager: PathManager::new(project_root),
            shell_timeout_secs: SHELL_TIMEOUT_SECS,
            shell_max_output_bytes: MAX_SHELL_OUTPUT_BYTES,
        }
    }

    pub fn from_config(project_root: PathBuf, config: &ToolsConfig) -> Self {
        ExecuteShellTool {
            path_manager: PathManager::new(project_root),
            shell_timeout_secs: config.shell_timeout_secs,
            shell_max_output_bytes: config.shell_max_output_bytes,
        }
    }
}

impl ToolExecutor for ExecuteShellTool {
    fn name(&self) -> &str {
        "execute_shell"
    }

    fn description(&self) -> &str {
        "Execute restricted read-only shell commands"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: ExecuteShellParams = serde_json::from_value(input.params)
            .map_err(|e| ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e)))?;

        let working_dir = self.path_manager.validate(&params.working_dir)?;

        if !working_dir.is_dir() {
            return Err(ToolError::new(
                ErrorCode::PathNotDirectory,
                format!("Working directory is not a directory: {}", params.working_dir),
            ));
        }

        ShellSecurity::validate_command(&params.command)
            .map_err(|(code, msg)| ToolError::new(code, msg))?;

        let final_command = ShellSecurity::inject_exclude_paths(&params.command, &params.exclude_paths);

        let mut working_dir_str = working_dir.to_string_lossy().to_string();
        // Windows canonicalize() returns \\?\ prefixed paths which some tools don't understand
        #[cfg(target_os = "windows")]
        {
            if working_dir_str.starts_with("\\\\?\\") {
                working_dir_str = working_dir_str[4..].to_string();
            }
        }

        let result = ShellExecutorFFI::execute(
            &final_command,
            &working_dir_str,
            self.shell_timeout_secs as i32,
            self.shell_max_output_bytes,
        );

        match result {
            Ok(shell_output) => {
                let truncated = shell_output.output_truncated;
                let output = ExecuteShellOutput {
                    success: shell_output.success,
                    output: shell_output.output,
                    error: if shell_output.success { None } else { Some(format!("Exit code: {}", shell_output.exit_code)) },
                };
                Ok(ToolOutput::new(serde_json::to_value(output).unwrap())
                    .with_truncated(truncated))
            }
            Err(shell_error) => {
                let code = match shell_error.error_code {
                    1 => ErrorCode::ShellCmdNotAllowed,
                    2 => ErrorCode::ShellDangerousOperator,
                    4 => ErrorCode::ExecutionTimeout,
                    _ => ErrorCode::ShellExecutionFailed,
                };
                Err(ToolError::new(code, shell_error.message))
            }
        }
    }
}
