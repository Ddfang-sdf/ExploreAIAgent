use std::path::{Component, Path, PathBuf};

use super::error::{ErrorCode, ToolError};

pub struct PathManager {
    project_root: PathBuf,
}

impl PathManager {
    pub fn new(project_root: PathBuf) -> Self {
        let canonical_root = project_root.canonicalize().unwrap_or(project_root);
        PathManager {
            project_root: canonical_root,
        }
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Validate a raw (relative) path against the project root.
    ///
    /// Uses component-level resolution to detect path traversal (`..`)
    /// before attempting OS canonicalization, so that non-existent paths
    /// that escape the project root are correctly rejected with
    /// `PATH_OUTSIDE_ROOT` rather than `PATH_NOT_FOUND`.
    pub fn validate(&self, raw_path: &str) -> Result<PathBuf, ToolError> {
        let raw_path = raw_path.trim();

        if raw_path.is_empty() || raw_path == "." {
            return Ok(self.project_root.clone());
        }

        let path = Path::new(raw_path);
        if path.is_absolute() {
            return Err(ToolError::new(
                ErrorCode::PathOutsideRoot,
                format!("Absolute path not allowed: {}", raw_path),
            ));
        }

        // --- Component-level resolution to detect `..` traversal ---
        let mut resolved = self.project_root.clone();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    if !resolved.pop() {
                        return Err(ToolError::new(
                            ErrorCode::PathOutsideRoot,
                            format!("Path escapes project root: {}", raw_path),
                        ));
                    }
                }
                Component::Normal(c) => {
                    resolved.push(c);
                }
                Component::CurDir => {}
                Component::Prefix(_) | Component::RootDir => {
                    return Err(ToolError::new(
                        ErrorCode::PathOutsideRoot,
                        format!("Path escapes project root: {}", raw_path),
                    ));
                }
            }
        }

        if !resolved.starts_with(&self.project_root) {
            return Err(ToolError::new(
                ErrorCode::PathOutsideRoot,
                format!("Path escapes project root: {}", raw_path),
            ));
        }

        // --- OS-level canonicalization for existence check ---
        let canonical = resolved.canonicalize().map_err(|_| {
            ToolError::new(
                ErrorCode::PathNotFound,
                format!("Path not found: {}", raw_path),
            )
        })?;

        if !canonical.starts_with(&self.project_root) {
            return Err(ToolError::new(
                ErrorCode::PathOutsideRoot,
                format!("Path escapes project root: {}", raw_path),
            ));
        }

        Ok(canonical)
    }
}
