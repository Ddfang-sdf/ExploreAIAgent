mod common;

use explore_ai_agent::common::error::ErrorCode;
use explore_ai_agent::common::path_manager::PathManager;

fn setup_pm() -> (tempfile::TempDir, PathManager) {
    let fixture = common::create_test_fixture();
    let pm = PathManager::new(fixture.path().to_path_buf());
    (fixture, pm)
}

/// PM-001: Normal relative path
#[test]
fn pm_001_normal_relative_path() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("src/main.rs");
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert!(resolved.ends_with("src/main.rs") || resolved.ends_with("src\\main.rs"));
    assert!(resolved.starts_with(pm.project_root()));
}

/// PM-002: Current directory "."
#[test]
fn pm_002_current_dir() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate(".");
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.canonicalize().unwrap(), pm.project_root().canonicalize().unwrap());
}

/// PM-003: Empty string
#[test]
fn pm_003_empty_string() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("");
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.canonicalize().unwrap(), pm.project_root().canonicalize().unwrap());
}

/// PM-004: Single level .. traversal
#[test]
fn pm_004_single_level_traversal() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("src/../../etc");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathOutsideRoot);
}

/// PM-005: Multi-level .. traversal
#[test]
fn pm_005_multi_level_traversal() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("../../../etc/passwd");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathOutsideRoot);
}

/// PM-006: Mixed . and ..
#[test]
fn pm_006_mixed_dots() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("src/./utils/../main.rs");
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert!(resolved.ends_with("main.rs"));
    assert!(resolved.starts_with(pm.project_root()));
}

/// PM-007: Absolute path input
#[test]
fn pm_007_absolute_path() {
    let (_fixture, pm) = setup_pm();

    #[cfg(unix)]
    let result = pm.validate("/etc/passwd");
    #[cfg(windows)]
    let result = pm.validate("C:\\Windows");

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathOutsideRoot);
}

/// PM-008: Path with spaces
#[test]
fn pm_008_path_with_spaces() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("special chars dir/file with spaces.txt");
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert!(resolved.exists());
}

/// PM-009: Nonexistent path
#[test]
fn pm_009_nonexistent_path() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("nonexistent/path");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::PathNotFound);
}

// --- Additional PathManager tests ---

#[test]
fn pm_validate_stays_within_root() {
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("src");
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert!(resolved.starts_with(pm.project_root()));
}

#[test]
fn pm_reject_traversal_via_symlink_simulation() {
    // This test validates the conceptual check; actual symlink testing is manual (MT-002)
    let (_fixture, pm) = setup_pm();
    let result = pm.validate("../");
    assert!(result.is_err());
}
