use std::env;
use std::fs;
use std::path::Path;

use serde::Deserialize;

// ============================================================================
// Default value functions
// ============================================================================

fn default_api_mode() -> String { "chat".to_string() }
fn default_base_url() -> String { "https://api.deepseek.com/v1".to_string() }
fn default_model() -> String { "deepseek-chat".to_string() }
fn default_max_retries() -> usize { 10 }
fn default_thinking() -> bool { false }

fn default_token_threshold() -> usize { 12000 }
fn default_token_target_ratio() -> f64 { 0.40 }
fn default_refiner_summary_ratio() -> f64 { 0.25 }
fn default_max_tool_calls() -> usize { 75 }
fn default_loop_warning_threshold() -> usize { 3 }

fn default_round_threshold() -> usize { 10 }
fn default_conv_token_threshold() -> usize { 2000 }

fn default_shell_timeout() -> u32 { 30 }
fn default_shell_max_output() -> usize { 10240 }
fn default_shell_max_lines() -> usize { 500 }

fn default_record_max_chars() -> usize { 8000 }
fn default_min_remaining() -> usize { 5 }

fn default_log_level() -> String { "info".to_string() }

fn default_workspace_path() -> String { "./workspace".to_string() }

fn default_timeout() -> u64 { 120 }

// ============================================================================
// Config structs
// ============================================================================

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InitScriptConfig {
    pub enabled: bool,

    pub script_path: String,

    #[serde(default = "default_timeout")]
    pub timeout_sec: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default = "default_workspace_path")]
    pub path: String,

    #[serde(default)]
    pub init_script: Option<InitScriptConfig>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        WorkspaceConfig {
            path: default_workspace_path(),
            init_script: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_api_mode")]
    pub api_mode: String,

    #[serde(default = "default_base_url")]
    pub base_url: String,

    #[serde(default)]
    pub api_key: String,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_max_retries")]
    pub max_retries: usize,

    #[serde(default = "default_thinking")]
    pub thinking: bool,

    /// Model total context window in tokens (e.g., 128000 for DeepSeek V3).
    /// Used to calculate the auto-compact trigger: `context_limit - max_output_tokens - 20000`.
    /// If not set, fall back to a round-based threshold.
    #[serde(default)]
    pub context_limit: Option<usize>,

    /// Model max output tokens. If not set, defaults to 8192.
    #[serde(default)]
    pub max_output_tokens: Option<usize>,

    /// API protocol: "openai" (default) or "anthropic".
    #[serde(default = "default_api_protocol")]
    pub api_protocol: String,
}

fn default_api_protocol() -> String { "openai".to_string() }

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            api_mode: default_api_mode(),
            base_url: default_base_url(),
            api_key: String::new(),
            model: default_model(),
            max_retries: default_max_retries(),
            thinking: default_thinking(),
            context_limit: None,
            max_output_tokens: None,
            api_protocol: default_api_protocol(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExplorationConfig {
    #[serde(default = "default_token_threshold")]
    pub token_threshold: usize,

    #[serde(default = "default_token_target_ratio")]
    pub token_target_ratio: f64,

    #[serde(default = "default_refiner_summary_ratio")]
    pub refiner_summary_token_ratio: f64,

    /// Override the formula-based compact threshold. When set, compaction triggers
    /// at this token count regardless of context_limit/max_output_tokens.
    /// When None (default), threshold = context_limit - max_output_tokens - 20000.
    #[serde(default)]
    pub compact_token_threshold: Option<usize>,
}

impl Default for ExplorationConfig {
    fn default() -> Self {
        ExplorationConfig {
            token_threshold: default_token_threshold(),
            token_target_ratio: default_token_target_ratio(),
            refiner_summary_token_ratio: default_refiner_summary_ratio(),
            compact_token_threshold: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeepExplorerConfig {
    #[serde(default = "default_enable")]
    pub enable: bool,

    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls: usize,

    #[serde(default = "default_loop_warning_threshold")]
    pub loop_warning_threshold: usize,

    #[serde(default = "default_token_threshold")]
    pub token_threshold: usize,

    #[serde(default = "default_token_target_ratio")]
    pub token_target_ratio: f64,
}

fn default_enable() -> bool { true }

#[derive(Debug, Clone, Deserialize)]
pub struct FastExploreConfig {
    #[serde(default = "default_enable")]
    pub enable: bool,
}

impl Default for FastExploreConfig {
    fn default() -> Self {
        FastExploreConfig { enable: true }
    }
}

impl Default for DeepExplorerConfig {
    fn default() -> Self {
        DeepExplorerConfig {
            enable: true,
            max_tool_calls: default_max_tool_calls(),
            loop_warning_threshold: default_loop_warning_threshold(),
            token_threshold: default_token_threshold(),
            token_target_ratio: default_token_target_ratio(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConversationConfig {
    #[serde(default = "default_round_threshold")]
    pub round_threshold: usize,

    #[serde(default = "default_conv_token_threshold")]
    pub token_threshold: usize,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        ConversationConfig {
            round_threshold: default_round_threshold(),
            token_threshold: default_conv_token_threshold(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_shell_timeout")]
    pub shell_timeout_secs: u32,

    #[serde(default = "default_shell_max_output")]
    pub shell_max_output_bytes: usize,

    #[serde(default = "default_shell_max_lines")]
    pub shell_max_output_lines: usize,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        ToolsConfig {
            shell_timeout_secs: default_shell_timeout(),
            shell_max_output_bytes: default_shell_max_output(),
            shell_max_output_lines: default_shell_max_lines(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextConfig {
    #[serde(default = "default_record_max_chars")]
    pub record_max_chars: usize,

    #[serde(default = "default_min_remaining")]
    pub min_remaining_records: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        ContextConfig {
            record_max_chars: default_record_max_chars(),
            min_remaining_records: default_min_remaining(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        LoggingConfig {
            level: default_log_level(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,

    #[serde(default)]
    pub exploration: ExplorationConfig,

    #[serde(default)]
    pub deep_explorer: DeepExplorerConfig,

    #[serde(default)]
    pub fast_explore: FastExploreConfig,

    #[serde(default)]
    pub conversation: ConversationConfig,

    #[serde(default)]
    pub workspace: WorkspaceConfig,

    #[serde(default)]
    pub tools: ToolsConfig,

    #[serde(default)]
    pub context: ContextConfig,

    #[serde(default)]
    pub logging: LoggingConfig,
}

// ============================================================================
// Public API
// ============================================================================

impl AppConfig {
    pub fn load(config_path: Option<&str>) -> Result<AppConfig, String> {
        // Step 1: determine config file path
        // Priority: explicit path > env var > ./config.yaml > ./config.yml > defaults
        let path = if let Some(p) = config_path {
            p.to_string()
        } else if let Ok(env_path) = env::var("EXPLORE_CONFIG_PATH") {
            env_path
        } else if Path::new("./config.yaml").exists() {
            "./config.yaml".to_string()
        } else if Path::new("./config.yml").exists() {
            "./config.yml".to_string()
        } else {
            // No config file — use all defaults
            return Ok(AppConfig::default());
        };

        // Step 2: read and parse YAML
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read config file '{}': {}", path, e))?;

        let mut config: AppConfig = serde_yaml::from_str(&content)
            .map_err(|e| format!("Failed to parse config file '{}': {}", path, e))?;

        // Step 3: apply environment variable overrides
        apply_env_overrides(&mut config);

        Ok(config)
    }
}

fn apply_env_overrides(config: &mut AppConfig) {
    // LLM
    if let Ok(v) = env::var("EXPLORE_LLM__API_KEY") { config.llm.api_key = v; }
    if let Ok(v) = env::var("EXPLORE_LLM__BASE_URL") { config.llm.base_url = v; }
    if let Ok(v) = env::var("EXPLORE_LLM__MODEL") { config.llm.model = v; }
    if let Ok(v) = env::var("EXPLORE_LLM__API_MODE") { config.llm.api_mode = v; }
    // Exploration
    if let Ok(v) = env::var("EXPLORE_EXPLORATION__TOKEN_THRESHOLD") {
        if let Ok(n) = v.parse() { config.exploration.token_threshold = n; }
    }
    // Deep Explorer
    if let Ok(v) = env::var("EXPLORE_DEEP_EXPLORER__MAX_TOOL_CALLS") {
        if let Ok(n) = v.parse() { config.deep_explorer.max_tool_calls = n; }
    }
    if let Ok(v) = env::var("EXPLORE_DEEP_EXPLORER__LOOP_WARNING_THRESHOLD") {
        if let Ok(n) = v.parse() { config.deep_explorer.loop_warning_threshold = n; }
    }
    // Tools
    if let Ok(v) = env::var("EXPLORE_TOOLS__SHELL_TIMEOUT_SECS") {
        if let Ok(n) = v.parse() { config.tools.shell_timeout_secs = n; }
    }
    // Context
    if let Ok(v) = env::var("EXPLORE_CONTEXT__RECORD_MAX_CHARS") {
        if let Ok(n) = v.parse() { config.context.record_max_chars = n; }
    }
    // Workspace
    if let Ok(v) = env::var("EXPLORE_WORKSPACE__PATH") { config.workspace.path = v; }
}
