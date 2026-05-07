use std::path::PathBuf;
use std::fs;
use serde::{Deserialize, Serialize};

use crate::common::error::{ErrorCode, ToolError};
use crate::common::models::{ToolInput, ToolOutput};
use crate::common::path_manager::PathManager;
use crate::tools::read_file::ReadFileTool;
use super::executor::ToolExecutor;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Directory,
    Code,
    Text,
    Config,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub lines_of_code: u32,
    pub comment_lines: u32,
    pub blank_lines: u32,
    pub top_level_declarations: u32,
    pub functions: u32,
    pub imports: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderComment {
    pub present: bool,
    pub lines: u32,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileInfoParams {
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfoOutput {
    pub success: bool,
    pub path: String,
    #[serde(rename = "type")]
    pub file_type: FileType,
    pub size: u64,
    pub lines: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<Stats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_comment: Option<HeaderComment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shebang: Option<String>,
}

pub struct FileInfoTool {
    path_manager: PathManager,
}

impl FileInfoTool {
    pub fn new(project_root: PathBuf) -> Self {
        FileInfoTool {
            path_manager: PathManager::new(project_root),
        }
    }

    pub fn detect_file_type(extension: Option<&str>) -> FileType {
        match extension {
            Some(ext) => {
                if CODE_EXTENSIONS.contains(&ext) {
                    FileType::Code
                } else if CONFIG_EXTENSIONS.contains(&ext) {
                    FileType::Config
                } else if TEXT_EXTENSIONS.contains(&ext) {
                    FileType::Text
                } else {
                    FileType::File
                }
            }
            None => FileType::File,
        }
    }

    pub fn detect_comment_style(extension: Option<&str>) -> Option<CommentStyle> {
        match extension {
            Some(ext) => match ext {
                "rs" | "java" | "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx"
                | "go" | "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx"
                | "swift" | "kt" | "kts" | "scala" | "cs" | "php" | "dart" => {
                    Some(CommentStyle {
                        single_line: Some("//".to_string()),
                        multi_line_start: Some("/*".to_string()),
                        multi_line_end: Some("*/".to_string()),
                    })
                }
                "py" | "rb" | "sh" | "bash" | "zsh" | "yaml" | "yml" | "toml"
                | "pl" | "pm" | "r" | "R" | "ex" | "exs" => {
                    Some(CommentStyle {
                        single_line: Some("#".to_string()),
                        multi_line_start: None,
                        multi_line_end: None,
                    })
                }
                "lua" => {
                    Some(CommentStyle {
                        single_line: Some("--".to_string()),
                        multi_line_start: Some("--[[".to_string()),
                        multi_line_end: Some("]]".to_string()),
                    })
                }
                "hs" => {
                    Some(CommentStyle {
                        single_line: Some("--".to_string()),
                        multi_line_start: Some("{-".to_string()),
                        multi_line_end: Some("-}".to_string()),
                    })
                }
                "ml" | "mli" => {
                    Some(CommentStyle {
                        single_line: None,
                        multi_line_start: Some("(*".to_string()),
                        multi_line_end: Some("*)".to_string()),
                    })
                }
                _ => None,
            }
            None => None,
        }
    }

    pub fn extract_header_comment(content: &str, style: &CommentStyle) -> HeaderComment {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return HeaderComment { present: false, lines: 0, content: String::new() };
        }

        let start_idx = if lines[0].starts_with("#!") { 1 } else { 0 };

        let mut comment_lines = Vec::new();
        let mut i = start_idx;
        let mut in_multi = false;

        while i < lines.len() && comment_lines.len() < 20 {
            let trimmed = lines[i].trim();

            if trimmed.is_empty() {
                if comment_lines.is_empty() {
                    i += 1;
                    continue;
                } else {
                    break;
                }
            }

            if in_multi {
                comment_lines.push(lines[i]);
                if let Some(ref end) = style.multi_line_end {
                    if trimmed.contains(end.as_str()) {
                        in_multi = false;
                    }
                }
                i += 1;
                continue;
            }

            if let Some(ref sl) = style.single_line {
                if trimmed.starts_with(sl.as_str()) {
                    comment_lines.push(lines[i]);
                    i += 1;
                    continue;
                }
            }

            if let Some(ref ms) = style.multi_line_start {
                if trimmed.starts_with(ms.as_str()) {
                    in_multi = true;
                    comment_lines.push(lines[i]);
                    if let Some(ref me) = style.multi_line_end {
                        if trimmed.contains(me.as_str()) && trimmed.find(me.as_str()).unwrap() > trimmed.find(ms.as_str()).unwrap() {
                            in_multi = false;
                        }
                    }
                    i += 1;
                    continue;
                }
            }

            break;
        }

        if comment_lines.is_empty() {
            HeaderComment { present: false, lines: 0, content: String::new() }
        } else {
            let count = comment_lines.len() as u32;
            let text = comment_lines.join("\n");
            HeaderComment { present: true, lines: count, content: text }
        }
    }

    pub fn analyze_code_stats(content: &str, style: &CommentStyle) -> Stats {
        let mut lines_of_code: u32 = 0;
        let mut comment_lines: u32 = 0;
        let mut blank_lines: u32 = 0;
        let mut top_level_declarations: u32 = 0;
        let mut functions: u32 = 0;
        let mut imports: u32 = 0;
        let mut in_multi_comment = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                blank_lines += 1;
                continue;
            }

            if in_multi_comment {
                comment_lines += 1;
                if let Some(ref me) = style.multi_line_end {
                    if trimmed.contains(me.as_str()) {
                        in_multi_comment = false;
                    }
                }
                continue;
            }

            if let Some(ref ms) = style.multi_line_start {
                if trimmed.starts_with(ms.as_str()) {
                    comment_lines += 1;
                    if let Some(ref me) = style.multi_line_end {
                        if !trimmed.contains(me.as_str()) || trimmed.find(me.as_str()).unwrap() <= trimmed.find(ms.as_str()).unwrap() {
                            in_multi_comment = true;
                        }
                    } else {
                        in_multi_comment = true;
                    }
                    continue;
                }
            }

            if let Some(ref sl) = style.single_line {
                if trimmed.starts_with(sl.as_str()) {
                    comment_lines += 1;
                    continue;
                }
            }

            lines_of_code += 1;

            if trimmed.starts_with("use ") || trimmed.starts_with("import ")
                || trimmed.starts_with("from ") || trimmed.starts_with("require")
                || trimmed.starts_with("#include") {
                imports += 1;
            }

            let decl_keywords = ["class ", "struct ", "enum ", "interface ", "trait ",
                "fn ", "def ", "function ", "pub fn ", "pub struct ",
                "pub enum ", "pub trait ", "pub const ", "const "];
            let is_indented = line.starts_with(' ') || line.starts_with('\t');
            if !is_indented {
                for kw in &decl_keywords {
                    if trimmed.starts_with(kw) {
                        top_level_declarations += 1;
                        break;
                    }
                }
            }

            let fn_patterns = ["fn ", "def ", "function ", "func "];
            for pat in &fn_patterns {
                if trimmed.contains(pat) && (trimmed.contains('(') || trimmed.contains('{')) {
                    functions += 1;
                    break;
                }
            }
        }

        Stats {
            lines_of_code,
            comment_lines,
            blank_lines,
            top_level_declarations,
            functions,
            imports,
        }
    }

    pub fn extract_shebang(content: &str) -> Option<String> {
        let first_line = content.lines().next()?;
        if first_line.starts_with("#!") {
            Some(first_line.to_string())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommentStyle {
    pub single_line: Option<String>,
    pub multi_line_start: Option<String>,
    pub multi_line_end: Option<String>,
}

impl ToolExecutor for FileInfoTool {
    fn name(&self) -> &str {
        "file_info"
    }

    fn description(&self) -> &str {
        "Get file metadata and code statistics"
    }

    fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: FileInfoParams = serde_json::from_value(input.params)
            .map_err(|e| ToolError::new(ErrorCode::InternalError, format!("Invalid params: {}", e)))?;

        let resolved = self.path_manager.validate(&params.file)?;

        let canonical_root = self.path_manager.project_root().canonicalize()
            .map_err(|e| ToolError::new(ErrorCode::InternalError, e.to_string()))?;
        let rel_path = resolved.strip_prefix(&canonical_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| params.file.clone());

        let metadata = fs::metadata(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ToolError::new(ErrorCode::PermissionDenied, format!("Permission denied: {}", params.file))
            } else {
                ToolError::new(ErrorCode::InternalError, e.to_string())
            }
        })?;

        if metadata.is_dir() {
            let output = FileInfoOutput {
                success: true,
                path: rel_path,
                file_type: FileType::Directory,
                size: 0,
                lines: 0,
                stats: None,
                header_comment: None,
                shebang: None,
            };
            return Ok(ToolOutput::new(serde_json::to_value(output).unwrap()));
        }

        let size = metadata.len();
        let ext = resolved.extension().and_then(|e| e.to_str());
        let file_type = Self::detect_file_type(ext);

        let raw_bytes = fs::read(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ToolError::new(ErrorCode::PermissionDenied, format!("Permission denied: {}", params.file))
            } else {
                ToolError::new(ErrorCode::InternalError, e.to_string())
            }
        })?;

        if ReadFileTool::is_binary_file(&raw_bytes) {
            let output = FileInfoOutput {
                success: true,
                path: rel_path,
                file_type: FileType::File,
                size,
                lines: 0,
                stats: None,
                header_comment: None,
                shebang: None,
            };
            return Ok(ToolOutput::new(serde_json::to_value(output).unwrap()));
        }

        let text = match std::str::from_utf8(&raw_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => String::from_utf8_lossy(&raw_bytes).to_string(),
        };

        let total_lines = if text.is_empty() { 0 } else { text.lines().count() as u32 };

        let (stats, header_comment, shebang) = if file_type == FileType::Code {
            let comment_style = Self::detect_comment_style(ext);
            let shebang = Self::extract_shebang(&text);

            match comment_style {
                Some(ref cs) => {
                    let stats = Self::analyze_code_stats(&text, cs);
                    let hc = Self::extract_header_comment(&text, cs);
                    (Some(stats), Some(hc), shebang)
                }
                None => {
                    (None, None, shebang)
                }
            }
        } else {
            (None, None, None)
        };

        let output = FileInfoOutput {
            success: true,
            path: rel_path,
            file_type,
            size,
            lines: total_lines,
            stats,
            header_comment,
            shebang,
        };

        Ok(ToolOutput::new(serde_json::to_value(output).unwrap()))
    }
}

pub const CODE_EXTENSIONS: &[&str] = &[
    "rs", "java", "py", "go", "js", "jsx", "mjs", "cjs",
    "ts", "tsx", "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx",
    "cs", "rb", "php", "swift", "kt", "kts", "scala",
    "sh", "bash", "zsh", "lua", "pl", "pm", "r", "R", "dart",
    "ex", "exs", "hs", "ml", "mli",
];

pub const CONFIG_EXTENSIONS: &[&str] = &[
    "json", "yaml", "yml", "toml", "xml", "ini", "cfg", "properties", "env",
];

pub const TEXT_EXTENSIONS: &[&str] = &[
    "md", "txt", "rst", "csv", "log",
];
