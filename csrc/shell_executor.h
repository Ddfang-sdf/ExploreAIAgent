/**
 * shell_executor - execute_shell 核心执行引擎
 *
 * 使用 C 实现以便进行底层系统调用控制：
 * - Unix: fork/exec + pipe 非阻塞读取
 * - Windows: CreatePipe + CreateProcess
 */

#ifndef SHELL_EXECUTOR_H
#define SHELL_EXECUTOR_H

#include <stddef.h>

/**
 * ShellResult 结构体
 *
 * | 字段             | 类型   | 说明                                          |
 * | success          | int    | 1=成功, 0=失败                                |
 * | output           | char*  | 命令输出（堆分配，调用方需通过 shell_result_free 释放）|
 * | output_len       | size_t | output 的实际字节长度                          |
 * | output_truncated | int    | 1=输出因超过 max_output_bytes 被截断, 0=完整输出|
 * | error            | char*  | 错误信息（堆分配）                             |
 * | error_code       | int    | 0=成功, 1=白名单拒绝, 2=操作符拒绝, 3=执行失败, 4=超时 |
 * | exit_code        | int    | 子进程退出码（仅 success=1 时有意义）           |
 */
typedef struct {
    int success;
    char *output;
    size_t output_len;
    int output_truncated;
    char *error;
    int error_code;
    int exit_code;
} ShellResult;

/**
 * 主入口：安全检查 + 管道非阻塞执行
 *
 * @param command          要执行的 Shell 命令
 * @param working_dir      命令执行目录（已校验的绝对路径）
 * @param timeout_sec      执行超时（秒）
 * @param max_output_bytes 输出截断上限（字节）
 * @return ShellResult     执行结果，调用方必须通过 shell_result_free 释放
 */
ShellResult shell_execute(const char *command, const char *working_dir,
                          int timeout_sec, size_t max_output_bytes);

/**
 * 白名单校验
 *
 * 提取 command 首个 token 作为主命令，检查是否在白名单中。
 *
 * @param command 完整命令字符串
 * @return 0=通过, 非0=拒绝
 */
int whitelist_check(const char *command);

/**
 * 危险操作符检测
 *
 * 扫描命令中的危险模式：输出重定向、命令替换、命令分隔符等。
 *
 * @param command 完整命令字符串
 * @return 0=通过, 非0=拒绝
 */
int operator_check(const char *command);

/**
 * 释放 ShellResult 中的动态内存
 *
 * @param result 指向待释放的 ShellResult
 */
void shell_result_free(ShellResult *result);

#endif /* SHELL_EXECUTOR_H */
