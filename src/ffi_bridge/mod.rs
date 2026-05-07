use std::ffi::{CString, CStr};
use std::os::raw::{c_char, c_int};

#[repr(C)]
pub struct ShellResultC {
    pub success: c_int,
    pub output: *mut c_char,
    pub output_len: usize,
    pub output_truncated: c_int,
    pub error: *mut c_char,
    pub error_code: c_int,
    pub exit_code: c_int,
}

extern "C" {
    pub fn shell_execute(
        command: *const c_char,
        working_dir: *const c_char,
        timeout_sec: c_int,
        max_output_bytes: usize,
    ) -> ShellResultC;

    pub fn whitelist_check(command: *const c_char) -> c_int;
    pub fn operator_check(command: *const c_char) -> c_int;
    pub fn shell_result_free(result: *mut ShellResultC);
}

#[derive(Debug, Clone)]
pub struct ShellOutput {
    pub success: bool,
    pub output: String,
    pub output_truncated: bool,
    pub exit_code: i32,
}

#[derive(Debug, Clone)]
pub struct ShellError {
    pub error_code: i32,
    pub message: String,
}

pub struct ShellExecutorFFI;

impl ShellExecutorFFI {
    pub fn execute(
        command: &str,
        working_dir: &str,
        timeout_sec: i32,
        max_output_bytes: usize,
    ) -> Result<ShellOutput, ShellError> {
        let c_command = CString::new(command).map_err(|_| ShellError {
            error_code: 3,
            message: "Command contains null byte".to_string(),
        })?;

        let c_working_dir = CString::new(working_dir).map_err(|_| ShellError {
            error_code: 3,
            message: "Working directory contains null byte".to_string(),
        })?;

        let mut result = unsafe {
            shell_execute(
                c_command.as_ptr(),
                c_working_dir.as_ptr(),
                timeout_sec as c_int,
                max_output_bytes,
            )
        };

        let output_str = if !result.output.is_null() {
            unsafe {
                let slice = std::slice::from_raw_parts(
                    result.output as *const u8,
                    result.output_len,
                );
                String::from_utf8_lossy(slice).into_owned()
            }
        } else {
            String::new()
        };

        let error_str = if !result.error.is_null() {
            unsafe { CStr::from_ptr(result.error) }
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };

        let success = result.success != 0;
        let output_truncated = result.output_truncated != 0;
        let exit_code = result.exit_code as i32;
        let error_code = result.error_code as i32;

        unsafe {
            shell_result_free(&mut result as *mut ShellResultC);
        }

        if error_code != 0 && !success {
            Err(ShellError {
                error_code,
                message: error_str,
            })
        } else {
            Ok(ShellOutput {
                success,
                output: output_str,
                output_truncated,
                exit_code,
            })
        }
    }
}
