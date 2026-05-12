use std::path::{Path, PathBuf};
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

// =========================================================================
// Redirect safety helpers
// =========================================================================

/// Check if a redirect target is safe:
/// - fd redirects (&1, &2 etc.) → safe (2>&1 style)
/// - /dev/null, /dev/fd/*, /dev/std*, /proc/*, /sys/* → safe
/// - Absolute path outside workspace → safe (temp files)
/// - Relative path → blocked (would write inside workspace)
fn is_redirect_safe(target: &str, project_root: &Path) -> bool {
    // fd redirect (e.g., `&1` in `2>&1`)
    if target.starts_with('&') {
        return true;
    }
    // /dev/null and related
    if target == "/dev/null"
        || target.starts_with("/dev/fd/")
        || target == "/dev/stdout"
        || target == "/dev/stderr"
    {
        return true;
    }
    // Absolute path (Unix)
    if target.starts_with('/') {
        let target_path = Path::new(target);
        // Canonicalize to check if inside workspace
        if let Ok(canon_target) = target_path.canonicalize() {
            if let Ok(canon_root) = project_root.canonicalize() {
                return !canon_target.starts_with(&canon_root);
            }
        }
        // Can't canonicalize (target may not exist yet — typical for redirect):
        // allow known-safe system paths, block unknown paths under project_root
        if target.starts_with("/tmp/")
            || target.starts_with("/dev/")
            || target.starts_with("/proc/")
            || target.starts_with("/sys/")
            || target.starts_with("/var/tmp/")
            || target.starts_with("/run/")
        {
            return true;
        }
        // Unknown absolute path: check if it starts with project_root as string
        let root_str = project_root.to_string_lossy();
        if target.starts_with(root_str.as_ref()) {
            return false; // under workspace → block
        }
        return true; // absolute, not under workspace → allow
    }
    // Windows absolute path (C:\...)
    if target.len() >= 2 && target.as_bytes().get(1) == Some(&b':') {
        let target_path = Path::new(target);
        if let Ok(canon_target) = target_path.canonicalize() {
            if let Ok(canon_root) = project_root.canonicalize() {
                return !canon_target.starts_with(&canon_root);
            }
        }
        return true; // can't canonicalize → allow
    }
    // Relative path → writes to workspace → block
    false
}

/// Split command by `;`, `&&`, `||` (outside quotes).
/// Returns sub-command segments for individual validation.
fn split_commands(command: &str) -> Vec<&str> {
    let chars: Vec<char> = command.chars().collect();
    let len = chars.len();
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;

    while i < len {
        if chars[i] == '\'' || chars[i] == '"' {
            let quote = chars[i];
            i += 1;
            while i < len {
                if quote == '"' && chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else if chars[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            continue;
        }

        if chars[i] == ';' {
            segments.push(&command[start..i]);
            i += 1;
            start = i;
            continue;
        }
        if i + 1 < len && chars[i] == '&' && chars[i + 1] == '&' {
            segments.push(&command[start..i]);
            i += 2;
            start = i;
            continue;
        }
        if i + 1 < len && chars[i] == '|' && chars[i + 1] == '|' {
            segments.push(&command[start..i]);
            i += 2;
            start = i;
            continue;
        }

        i += 1;
    }

    segments.push(&command[start..]);
    segments
}

impl ShellSecurity {
    pub const WHITELIST: &'static [&'static str] = &[
        "cat", "head", "tail", "less",
        "grep", "egrep", "fgrep", "find",
        "ls", "tree",
        "wc", "sort", "uniq", "cut", "tr",
        "awk", "sed",
        "file", "stat",
        "echo", "xargs",
        // Windows equivalents
        "type", "dir", "findstr",
    ];

    pub fn check_whitelist(command: &str) -> Result<(), String> {
        let trimmed = command.trim();
        // Extract the actual command, skipping leading redirects like
        // `2>/dev/null find .` or `2>&1 grep ...` (valid shell syntax).
        let main_cmd = trimmed
            .split(char::is_whitespace)
            .skip_while(|w| Self::is_redirect_token(w))
            .next()
            .unwrap_or("");

        if Self::WHITELIST.contains(&main_cmd) {
            if main_cmd == "sed" {
                Self::check_sed_inplace(trimmed)?;
            }
            Ok(())
        } else {
            Err(format!("命令 '{}' 不在白名单中。可用命令：cat head tail less grep egrep fgrep find ls tree wc sort uniq cut tr awk sed file stat echo type dir findstr", main_cmd))
        }
    }

    pub fn check_dangerous_operators(command: &str, project_root: &Path) -> Result<(), String> {
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
                    return Err("禁止 tee — 可写入文件。如需查看内容请使用 cat/head/tail".to_string());
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

            // Output redirect >
            if chars[i] == '>' {
                if i + 1 < len && chars[i + 1] == '>' {
                    // >> still blocked — no legitimate read-only use
                    return Err("禁止追加重定向 >> — 仅支持只读操作".to_string());
                }
                // Extract redirect target inline (char-level, ASCII-safe for shell commands)
                let mut j = i + 1;
                while j < len && chars[j].is_whitespace() { j += 1; }
                if j < len {
                    let target: String = if chars[j] == '&' {
                        // fd redirect like &1 in 2>&1 — just signal it
                        "&".to_string()
                    } else {
                        let t_start = j;
                        while j < len && !chars[j].is_whitespace()
                            && chars[j] != '|' && chars[j] != ';' && chars[j] != '&'
                        {
                            j += 1;
                        }
                        chars[t_start..j].iter().collect()
                    };
                    if !is_redirect_safe(&target, project_root) {
                        return Err(format!(
                            "禁止重定向到工作区文件 '{}' — 此操作会修改项目文件。可改用 > /dev/null 抑制输出",
                            target
                        ));
                    }
                }
                // Skip past this > and its target to avoid re-processing
                i = j;
                continue;
            }

            // Path traversal
            if (i + 2 < len && chars[i] == '.' && chars[i+1] == '.' && chars[i+2] == '/')
                || (i + 2 < len && chars[i] == '.' && chars[i+1] == '.' && chars[i+2] == '\\')
            {
                return Err("禁止路径穿越 ../ — 仅允许项目目录内的文件".to_string());
            }

            i += 1;
        }

        // system(/exec( checks: these are dangerous even inside quotes
        if command.contains("system(") || command.contains("exec(") {
            return Err("禁止 system()/exec() — 即使引号内也不允许（可通过 awk system() 绕过白名单）".to_string());
        }

        // --- background & (not &&) ---
        let has_background = {
            let trimmed = command.trim();
            if trimmed.ends_with('&') {
                let prefix = trimmed[..trimmed.len() - 1].trim_end();
                !prefix.ends_with('&')
            } else {
                let re = regex::Regex::new(r"(?:^|[^>&])&(?:[^&]|$)").unwrap();
                let without_double = command.replace("&&", "");
                re.is_match(&without_double)
            }
        };
        if has_background {
            return Err("禁止后台执行 & — 仅允许前台同步执行".to_string());
        }

        Ok(())
    }

    pub fn check_sed_inplace(command: &str) -> Result<(), String> {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        for token in &tokens[1..] {
            if *token == "-i" || token.starts_with("-i") {
                return Err("禁止 sed -i 原地修改 — 仅允许只读操作".to_string());
            }
        }
        Ok(())
    }

    /// Split by `|` outside quotes. Returns pipeline segments.
    fn split_pipe_segments<'a>(command: &'a str) -> Vec<&'a str> {
        let chars: Vec<char> = command.chars().collect();
        let len = chars.len();
        let mut segments = Vec::new();
        let mut start = 0usize;
        let mut i = 0usize;

        while i < len {
            if chars[i] == '\'' || chars[i] == '"' {
                let quote = chars[i];
                i += 1;
                while i < len {
                    if quote == '"' && chars[i] == '\\' && i + 1 < len {
                        i += 2;
                    } else if chars[i] == quote {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            if chars[i] == '|' {
                segments.push(&command[start..i]);
                i += 1;
                start = i;
                continue;
            }
            i += 1;
        }
        segments.push(&command[start..]);
        segments
    }

    pub fn check_pipe_commands(command: &str, project_root: &Path) -> Result<(), String> {
        if !command.contains('|') {
            return Ok(());
        }

        let segments = Self::split_pipe_segments(command);
        for segment in &segments {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                continue;
            }
            let main_cmd = trimmed
                .split_whitespace()
                .skip_while(|w| Self::is_redirect_token(w))
                .next()
                .unwrap_or("");

            if main_cmd.is_empty() {
                continue; // only redirects in this segment, skip
            }
            if main_cmd == "tee" {
                return Err("tee command detected (dangerous: can write files)".to_string());
            }

            if !Self::WHITELIST.contains(&main_cmd) {
                return Err(format!("Pipe segment command '{}' is not in the whitelist", main_cmd));
            }

            if main_cmd == "sed" {
                Self::check_sed_inplace(trimmed)?;
            }

            Self::check_dangerous_operators(trimmed, project_root)?;
        }

        Ok(())
    }

    /// Returns true if a whitespace-delimited token is a shell redirect,
    /// not an actual command. Examples: `2>/dev/null`, `2>&1`, `>/dev/null`.
    fn is_redirect_token(token: &str) -> bool {
        // fd redirect: 2>, 1>, &>, >, 2>>, etc.
        if token.starts_with('>') || token.ends_with('>') || token.contains(">&") {
            return true;
        }
        // The target of a redirect: /dev/null, /dev/fd/*, &1, etc.
        if token == "/dev/null" || token.starts_with("/dev/fd/") || token.starts_with("&") {
            return true;
        }
        // Numbered fd: 2, 1 (followed by > in context — but we check token by token)
        false
    }

    pub fn validate_command(command: &str, project_root: &Path) -> Result<(), (ErrorCode, String)> {
        // Split by ;, &&, || into sub-commands, then validate each independently
        let segments = split_commands(command);
        for segment in &segments {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                continue;
            }

            Self::check_whitelist(trimmed)
                .map_err(|e| (ErrorCode::ShellCmdNotAllowed, e))?;

            Self::check_dangerous_operators(trimmed, project_root)
                .map_err(|e| (ErrorCode::ShellDangerousOperator, e))?;

            Self::check_pipe_commands(trimmed, project_root)
                .map_err(|e| {
                    if e.contains("not in the whitelist") {
                        (ErrorCode::ShellCmdNotAllowed, e)
                    } else {
                        (ErrorCode::ShellDangerousOperator, e)
                    }
                })?;
        }

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
    shell_max_output_lines: usize,
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
            shell_max_output_lines: MAX_SHELL_OUTPUT_LINES,
            shell_path: discover_shell(),
        }
    }

    pub fn from_config(project_root: PathBuf, config: &ToolsConfig) -> Self {
        ExecuteShellTool {
            path_manager: PathManager::new(project_root),
            shell_timeout_secs: config.shell_timeout_secs,
            shell_max_output_bytes: config.shell_max_output_bytes,
            shell_max_output_lines: config.shell_max_output_lines,
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

        ShellSecurity::validate_command(&params.command, self.path_manager.project_root())
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

                // Rust-layer line truncation
                let (trimmed, line_truncated) =
                    TruncationManager::truncate_lines(&output_text, self.shell_max_output_lines);
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
