pub const MAX_OUTPUT_BYTES: usize = 64 * 1024; // 64 KB
pub const MAX_SHELL_OUTPUT_BYTES: usize = 50 * 1024; // 50 KB
pub const MAX_SHELL_OUTPUT_LINES: usize = 2000;
pub const MAX_SEARCH_FILES_RESULTS: usize = 1000;
pub const MAX_SEARCH_CONTENT_RESULTS: usize = 500;
pub const MAX_LIST_DIR_ITEMS: usize = 1000;
pub const MAX_READ_FILE_LINES: usize = 2000;
pub const MAX_LARGE_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
pub const MAX_SEARCH_FILE_SIZE: u64 = 5 * 1024 * 1024; // 5 MB
pub const EXECUTION_TIMEOUT_SECS: u32 = 30;
pub const SHELL_TIMEOUT_SECS: u32 = 120;
pub const MAX_CONTEXT_LINES: usize = 5;
pub const RECORD_MAX_CHARS: usize = 8000;

pub struct TruncationManager;

impl TruncationManager {
    pub fn truncate_output(data: &[u8], max_bytes: usize) -> (Vec<u8>, bool) {
        if data.len() <= max_bytes {
            return (data.to_vec(), false);
        }
        let mut end = max_bytes;
        while end > 0 && data[end] & 0xC0 == 0x80 {
            end -= 1;
        }
        (data[..end].to_vec(), true)
    }

    pub fn truncate_lines(content: &str, max_lines: usize) -> (&str, bool) {
        let mut count = 0;
        for (i, ch) in content.char_indices() {
            if ch == '\n' {
                count += 1;
                if count >= max_lines {
                    return (&content[..i + 1], true);
                }
            }
        }
        (content, false)
    }
}
