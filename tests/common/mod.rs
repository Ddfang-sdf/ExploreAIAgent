use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Creates the standard test fixture directory structure as specified in
/// design document section 8.1.
///
/// Returns a TempDir (which cleans up on drop) containing:
/// - src/main.rs (30 lines Rust code, no header comment)
/// - src/lib.rs (50 lines Rust code, with header comment + use statements)
/// - src/utils/helper.py (20 lines Python code, with # comments)
/// - src/utils/config.yaml (YAML config file)
/// - tests/test_main.rs (Rust test file)
/// - tests/integration_test.java (Java test file)
/// - docs/readme.md (Markdown doc)
/// - .git/config (simulated .git dir)
/// - node_modules/fake_module/index.js (should be skipped)
/// - empty_dir/ (empty directory)
/// - binary_file.bin (binary file with NUL bytes)
/// - large_file.txt (>10 MB, filled with repeated lines)
/// - special chars dir/file with spaces.txt
/// - .hidden_file
pub fn create_test_fixture() -> TempDir {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let root = tmp.path();

    // --- src/main.rs (30 lines, no header comment, has fn main) ---
    create_dir(root, "src");
    let main_rs = generate_rust_main(30);
    write_file(root, "src/main.rs", &main_rs);

    // --- src/lib.rs (50 lines, with header comment, use statements, functions) ---
    let lib_rs = generate_rust_lib(50);
    write_file(root, "src/lib.rs", &lib_rs);

    // --- src/utils/helper.py (20 lines Python) ---
    create_dir(root, "src/utils");
    let helper_py = generate_python_helper(20);
    write_file(root, "src/utils/helper.py", &helper_py);

    // --- src/utils/config.yaml ---
    write_file(root, "src/utils/config.yaml", "server:\n  port: 8080\n  host: localhost\nlogging:\n  level: info\n");

    // --- tests/test_main.rs ---
    create_dir(root, "tests");
    write_file(root, "tests/test_main.rs", "#[test]\nfn test_example() {\n    assert!(true);\n}\n");

    // --- tests/integration_test.java ---
    write_file(root, "tests/integration_test.java", "import org.junit.Test;\n\npublic class IntegrationTest {\n    @Test\n    public void testSomething() {\n        assert true;\n    }\n}\n");

    // --- docs/readme.md ---
    create_dir(root, "docs");
    write_file(root, "docs/readme.md", "# Project\n\nThis is a test project.\n\n## Config\n\nSee config.yaml for settings.\n");

    // --- .git/config (simulated) ---
    create_dir(root, ".git");
    write_file(root, ".git/config", "[core]\n\trepositoryformatversion = 0\n");

    // --- node_modules/fake_module/index.js ---
    create_dir(root, "node_modules/fake_module");
    write_file(root, "node_modules/fake_module/index.js", "module.exports = {};\n");

    // --- empty_dir/ ---
    create_dir(root, "empty_dir");

    // --- binary_file.bin (contains NUL bytes) ---
    let mut binary_data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG-like header
    binary_data.extend_from_slice(&[0x00; 100]); // NUL bytes
    binary_data.extend_from_slice(b"some data");
    fs::write(root.join("binary_file.bin"), &binary_data).unwrap();

    // --- large_file.txt (> 10 MB) ---
    let line = "This is a repeated line for the large test file. It contains some words.\n";
    let repeat_count = (10 * 1024 * 1024 / line.len()) + 100;
    let large_content: String = line.repeat(repeat_count);
    fs::write(root.join("large_file.txt"), large_content).unwrap();

    // --- special chars dir/file with spaces.txt ---
    create_dir(root, "special chars dir");
    write_file(root, "special chars dir/file with spaces.txt", "Content in file with spaces in path.\n");

    // --- .hidden_file ---
    write_file(root, ".hidden_file", "hidden content\n");

    // --- gbk_file.txt (GBK-encoded content, not valid UTF-8) ---
    let gbk_bytes: Vec<u8> = vec![
        0xD6, 0xD0, 0xCE, 0xC4, 0xC4, 0xDA, 0xC8, 0xDD, // "中文内容" in GBK
        0xB2, 0xE2, 0xCA, 0xD4, 0x0A, // "测试\n" in GBK
    ];
    fs::write(root.join("gbk_file.txt"), &gbk_bytes).unwrap();

    tmp
}

/// Creates a fixture with many files for truncation testing.
#[allow(dead_code)]
pub fn create_many_files_fixture(count: usize, extension: &str) -> TempDir {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let root = tmp.path();
    create_dir(root, "bulk");
    for i in 0..count {
        let filename = format!("bulk/file_{:05}.{}", i, extension);
        write_file(root, &filename, &format!("content of file {}\n", i));
    }
    tmp
}

/// Creates a fixture with a file of N lines.
#[allow(dead_code)]
pub fn create_file_with_lines(filename: &str, line_count: usize) -> TempDir {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let mut content = String::new();
    for i in 1..=line_count {
        content.push_str(&format!("Line {} of the test file\n", i));
    }
    fs::write(tmp.path().join(filename), &content).unwrap();
    tmp
}

/// Creates a fixture with many matches for search_content truncation testing.
#[allow(dead_code)]
pub fn create_many_matches_fixture(file_count: usize, matches_per_file: usize) -> TempDir {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let root = tmp.path();
    create_dir(root, "matchdir");
    for i in 0..file_count {
        let mut content = String::new();
        for j in 0..matches_per_file {
            content.push_str(&format!("MATCH_TARGET line {} in file {}\n", j, i));
            content.push_str("non-matching line\n");
        }
        let filename = format!("matchdir/match_file_{:04}.txt", i);
        write_file(root, &filename, &content);
    }
    tmp
}

/// Creates a fixture directory with many items for list_dir truncation testing.
#[allow(dead_code)]
pub fn create_many_items_dir(count: usize) -> TempDir {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let root = tmp.path();
    create_dir(root, "bigdir");
    for i in 0..count {
        write_file(root, &format!("bigdir/item_{:05}.txt", i), "x");
    }
    tmp
}

fn create_dir(root: &Path, relative: &str) {
    fs::create_dir_all(root.join(relative)).expect(&format!("Failed to create dir: {}", relative));
}

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&path, content).expect(&format!("Failed to write: {}", relative));
}

fn generate_rust_main(lines: usize) -> String {
    let mut content = String::new();
    content.push_str("use std::io;\n");
    content.push_str("\n");
    content.push_str("fn main() {\n");
    content.push_str("    println!(\"Hello, world!\");\n");
    content.push_str("    let x = 42;\n");
    for i in 5..lines - 1 {
        content.push_str(&format!("    let _v{} = {};\n", i, i));
    }
    content.push_str("}\n");
    content
}

fn generate_rust_lib(lines: usize) -> String {
    let mut content = String::new();
    // 5 header comment lines
    content.push_str("// Library module for the project\n");
    content.push_str("// Contains utility functions\n");
    content.push_str("// Version: 1.0\n");
    content.push_str("// Author: Test\n");
    content.push_str("// License: MIT\n");
    // Blank line 1
    content.push_str("\n");
    // 3 import lines (code)
    content.push_str("use std::collections::HashMap;\n");
    content.push_str("use std::path::PathBuf;\n");
    content.push_str("use std::io::Result;\n");
    // Blank line 2
    content.push_str("\n");
    // Function 1 (3 code lines)
    content.push_str("pub fn helper_one() -> i32 {\n");
    content.push_str("    42\n");
    content.push_str("}\n");
    // Blank line 3
    content.push_str("\n");
    // Function 2 (3 code lines)
    content.push_str("pub fn helper_two(x: i32) -> i32 {\n");
    content.push_str("    x * 2\n");
    content.push_str("}\n");
    // So far: 5 comment + 3 blank + 9 code = 17 lines
    // Need: lines total with 42 code = 33 more code lines
    for i in 0..lines - 17 {
        content.push_str(&format!("const VAL_{}: i32 = {};\n", i, i));
    }
    content
}

fn generate_python_helper(lines: usize) -> String {
    let mut content = String::new();
    content.push_str("#!/usr/bin/env python3\n");
    content.push_str("# Helper utilities\n");
    content.push_str("# For testing purposes\n");
    content.push_str("\n");
    content.push_str("import os\n");
    content.push_str("import sys\n");
    content.push_str("\n");
    content.push_str("def helper_function(x):\n");
    content.push_str("    return x + 1\n");
    content.push_str("\n");
    content.push_str("def another_function():\n");
    content.push_str("    # This function does nothing useful\n");
    content.push_str("    pass\n");
    let current = content.lines().count();
    for i in current..lines {
        content.push_str(&format!("VAR_{} = {}\n", i, i));
    }
    content
}
