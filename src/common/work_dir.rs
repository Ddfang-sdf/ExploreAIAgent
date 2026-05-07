use std::fs;
use std::path::Path;
use std::process::Command;
use serde::Deserialize;

fn default_timeout() -> u64 {
    120
}

#[derive(Debug, Clone, Deserialize)]
pub struct InitScriptConfig {
    pub enabled: bool,

    pub script_path: String,

    #[serde(default = "default_timeout")]
    pub timeout_sec: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceConfig {
    pub path: String,

    #[serde(default)]
    pub init_script: Option<InitScriptConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceStatus {
    Ready { item_count: usize },
    EmptyWithoutScript,
    EmptyWithScript,
    ScriptExecutedButEmpty,
    ScriptExecutionFailed { exit_code: Option<i32>, stderr: String },
}

#[derive(Debug, Clone)]
pub enum InitError {
    ScriptFailed { exit_code: Option<i32>, stderr: String },
    Timeout,
}

pub struct WorkDirInitializer {
    config: WorkspaceConfig,
}

impl WorkDirInitializer {
    pub fn new(config: WorkspaceConfig) -> Self {
        WorkDirInitializer { config }
    }

    pub fn check_workspace(&self) -> Result<WorkspaceStatus, String> {
        let path = Path::new(&self.config.path);

        if !path.exists() {
            return Ok(if self.config.init_script.as_ref().map_or(false, |s| s.enabled) {
                WorkspaceStatus::EmptyWithScript
            } else {
                WorkspaceStatus::EmptyWithoutScript
            });
        }

        if !path.is_dir() {
            return Ok(if self.config.init_script.as_ref().map_or(false, |s| s.enabled) {
                WorkspaceStatus::EmptyWithScript
            } else {
                WorkspaceStatus::EmptyWithoutScript
            });
        }

        let entries: Vec<_> = fs::read_dir(path)
            .map_err(|e| format!("Failed to read workspace directory: {}", e))?
            .filter_map(|e| e.ok())
            .collect();

        let item_count = entries.len();

        if item_count == 0 {
            Ok(if self.config.init_script.as_ref().map_or(false, |s| s.enabled) {
                WorkspaceStatus::EmptyWithScript
            } else {
                WorkspaceStatus::EmptyWithoutScript
            })
        } else {
            Ok(WorkspaceStatus::Ready { item_count })
        }
    }

    pub fn run_init_script(&self, workspace_path: &str) -> Result<(), InitError> {
        let script_config = match &self.config.init_script {
            Some(s) if s.enabled => s,
            _ => return Ok(()),
        };

        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd.exe");
            c.args(["/C", &script_config.script_path, workspace_path]);
            c
        } else {
            let mut c = Command::new("/bin/sh");
            c.args([&script_config.script_path, workspace_path]);
            c
        };

        let output = cmd
            .output()
            .map_err(|e| InitError::ScriptFailed {
                exit_code: None,
                stderr: format!("Failed to execute script: {}", e),
            })?;

        if output.status.success() {
            Ok(())
        } else {
            Err(InitError::ScriptFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        }
    }
}
