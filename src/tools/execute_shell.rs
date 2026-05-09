use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::common::path_manager::PathManager;
use crate::common::config::ToolsConfig;
use crate::common::truncation::{MAX_SHELL_OUTPUT_BYTES, MAX_SHELL_OUTPUT_LINES, SHELL_TIMEOUT_SECS};
use crate::common::truncation::TruncationManager;
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
        let chars: Vec<char> = command.chars().collect();
        let len = chars.len();

        // Helper: advance past a quoted segment, returns index after closing quote
        let skip_quoted = |chars: &[char], mut i: usize, quote: char| -> usize {
            i += 1; // skip opening quote
            while i < chars.len() {
                if quote == '"' && chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2; // skip escaped char
                } else if chars[i] == quote {
                    return i + 1; // skip closing quote
                } else {
                    i += 1;
                }
            }
            i // unterminated quote, go to end
        };

        // tee token check (whole-word match outside quotes)
        {
            let tokens: Vec<&str> = command.split_whitespace().collect();
            for token in &tokens {
                let trimmed = token.trim_matches(&['\'', '"'][..]);
                if trimmed == "tee" {
                    return Err("tee command detected (dangerous: can write files)".to_string());
                }
            }
        }

        let mut i = 0;
        while i < len {
            // Skip quoted content entirely
            if chars[i] == '\'' || chars[i] == '"' {
                i = skip_quoted(&chars, i, chars[i]);
                continue;
            }

            // Command substitution
            if chars[i] == '$' && i + 1 < len && chars[i + 1] == '(' {
                return Err("Command substitution $() detected".to_string());
            }
            if chars[i] == '`' {
                return Err("Backtick command substitution detected".to_string());
            }

            // Command separator
            if chars[i] == ';' {
                return Err("Command separator ; detected".to_string());
            }

            // Logical operators
            if chars[i] == '&' && i + 1 < len && chars[i + 1] == '&' {
                return Err("Logical AND && detected".to_string());
            }
            if chars[i] == '|' && i + 1 < len && chars[i + 1] == '|' {
                return Err("Logical OR || detected".to_string());
            }

            // Output redirect >
            if chars[i] == '>' {
                if i + 1 < len && chars[i + 1] == '>' {
                    return Err("Append redirect >> detected".to_string());
                }
                return Err("Output redirect > detected".to_string());
            }

            // Path traversal
            if (i + 2 < len && chars[i] == '.' && chars[i+1] == '.' && chars[i+2] == '/')
                || (i + 2 < len && chars[i] == '.' && chars[i+1] == '.' && chars[i+2] == '\\')
            {
                return Err("Path traversal ../ detected".to_string());
            }

            i += 1;
        }

        // system(/exec( checks: these are dangerous even inside quotes
        // (awk's system() calls the OS directly, bypassing our whitelist)
        if command.contains("system(") || command.contains("exec(") {
            return Err("System/exec call detected in command".to_string());
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
    pub shell_path: String,
}

/// Discover the best available shell on this system.
fn discover_shell() -> String {
    let shells = discover_all_shells();
    shells.into_iter().next().unwrap_or_else(|| {
        #[cfg(target_os = "windows")] { "cmd.exe".to_string() }
        #[cfg(not(target_os = "windows"))] { "/bin/sh".to_string() }
    })
}

/// Collect all available shells sorted by capability (best first).
fn discover_all_shells() -> Vec<String> {
    let mut shells: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    #[cfg(target_os = "windows")]
    {
        // 1. Git Bash via git location (most reliable — OpenCode approach)
        if let Some(gb) = find_git_bash() {
            seen.insert(gb.clone());
            shells.push(gb);
        }

        // 2. Known full-featured bash paths
        for p in &[r"C:\Program Files\Git\bin\bash.exe", r"C:\msys64\usr\bin\bash.exe"] {
            if std::path::Path::new(p).exists() && seen.insert(p.to_string()) {
                shells.push(p.to_string());
            }
        }

        // 3. PATH scan: bash (prefer bin/ over usr\bin/), then pwsh
        if let Ok(path) = std::env::var("PATH") {
            let mut usr_bin_bash: Option<String> = None;
            for dir in std::env::split_paths(&path) {
                for name in &["bash.exe", "pwsh.exe"] {
                    let p = dir.join(name);
                    if !p.exists() { continue; }
                    let s = p.to_string_lossy().to_string();
                    if seen.contains(&s) { continue; }
                    seen.insert(s.clone());
                    if *name == "pwsh.exe" {
                        shells.push(s);
                    } else if !s.contains("usr\\bin") {
                        shells.push(s); // bin/bash preferred
                    } else {
                        usr_bin_bash = Some(s); // deferred
                    }
                }
            }
            if let Some(b) = usr_bin_bash { shells.push(b); }
        }

        // 4. PowerShell from known locations
        for p in &[r"C:\Program Files\PowerShell\7\pwsh.exe",
                   r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"] {
            if std::path::Path::new(p).exists() && seen.insert(p.to_string()) {
                shells.push(p.to_string());
            }
        }

        // 5. cmd.exe last
        shells.push("cmd.exe".to_string());
    }

    #[cfg(not(target_os = "windows"))]
    {
        for p in &["/bin/bash", "/bin/zsh", "/bin/sh"] {
            if std::path::Path::new(p).exists() && seen.insert(p.to_string()) {
                shells.push(p.to_string());
            }
        }
    }

    shells
}

/// Find Git Bash by locating git.exe and deriving bash path from it.
/// Mirrors OpenCode's gitbash(): `which git` → `../../bin/bash.exe`
#[cfg(target_os = "windows")]
fn find_git_bash() -> Option<String> {
    // Try "where git" to find git.exe, then resolve ../bin/bash.exe
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let git = dir.join("git.exe");
            if git.exists() {
                // Git Bash is at <git_root>/bin/bash.exe
                // git.exe is typically at <git_root>/cmd/git.exe or <git_root>/bin/git.exe
                for relative in &[r"..\bin\bash.exe", r"..\..\bin\bash.exe"] {
                    if let Ok(resolved) = git.canonicalize() {
                        let parent = resolved.parent()?;
                        let bash = parent.join(relative);
                        if let Ok(canon) = bash.canonicalize() {
                            if canon.exists() {
                                return Some(canon.to_string_lossy().to_string());
                            }
                        }
                        // Try without canonicalize (some paths may not resolve)
                        let bash2 = std::path::Path::new(&dir).join(relative);
                        if bash2.exists() {
                            return Some(bash2.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

impl ExecuteShellTool {
    pub fn new(project_root: PathBuf) -> Self {
        ExecuteShellTool {
            path_manager: PathManager::new(project_root),
            shell_timeout_secs: SHELL_TIMEOUT_SECS,
            shell_max_output_bytes: MAX_SHELL_OUTPUT_BYTES,
            shell_path: discover_shell(),
        }
    }

    pub fn from_config(project_root: PathBuf, config: &ToolsConfig) -> Self {
        ExecuteShellTool {
            path_manager: PathManager::new(project_root),
            shell_timeout_secs: config.shell_timeout_secs,
            shell_max_output_bytes: config.shell_max_output_bytes,
            shell_path: discover_shell(),
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
            &self.shell_path,
        );

        match result {
            Ok(shell_output) => {
                // C-layer byte truncation
                let mut truncated = shell_output.output_truncated;
                let mut output_text = shell_output.output;

                // Rust-layer line truncation (2000 lines, per design doc 7.3)
                let (trimmed, line_truncated) =
                    TruncationManager::truncate_lines(&output_text, MAX_SHELL_OUTPUT_LINES);
                if line_truncated {
                    output_text = trimmed.to_string();
                    truncated = true;
                }

                let output = ExecuteShellOutput {
                    success: shell_output.success,
                    output: output_text,
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
