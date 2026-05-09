/**
 * shell_executor - execute_shell 核心执行引擎
 *
 * Unix: fork/exec + pipe 非阻塞读取
 * Windows: CreatePipe + CreateProcess
 */

#include "shell_executor.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#ifdef _WIN32
#define strdup _strdup
#include <windows.h>
#else
#include <unistd.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <fcntl.h>
#include <signal.h>
#include <time.h>
#include <errno.h>
#include <sys/select.h>
#endif

/* ---- whitelist ---- */

static const char *WHITELIST[] = {
    "cat", "head", "tail", "less",
    "grep", "egrep", "fgrep", "find",
    "ls", "tree",
    "wc", "sort", "uniq", "cut", "tr",
    "awk", "sed",
    "file", "stat",
    "echo",
    /* Windows equivalents */
    "type", "dir", "findstr",
    NULL
};

int whitelist_check(const char *command) {
    if (!command) return 1;
    while (*command == ' ' || *command == '\t') command++;

    char cmd_buf[256];
    int i = 0;
    while (command[i] && command[i] != ' ' && command[i] != '\t'
           && command[i] != ';' && command[i] != '|' && command[i] != '&'
           && i < 255) {
        cmd_buf[i] = command[i];
        i++;
    }
    cmd_buf[i] = '\0';

    for (const char **w = WHITELIST; *w; w++) {
        if (strcmp(cmd_buf, *w) == 0) {
            /* sed -i check */
            if (strcmp(cmd_buf, "sed") == 0) {
                if (strstr(command, " -i") != NULL) {
                    return 1;
                }
            }
            return 0;
        }
    }
    return 1;
}

/**
 * Helper: advance past a quoted segment.
 * Returns pointer after the closing quote, or end of string if unterminated.
 */
static const char *skip_quoted(const char *p, char quote) {
    p++; /* skip opening quote */
    while (*p && *p != quote) {
        if (quote == '"' && *p == '\\' && *(p + 1)) p += 2; /* skip escaped char */
        else p++;
    }
    if (*p == quote) p++; /* skip closing quote */
    return p;
}

int operator_check(const char *command) {
    if (!command) return 1;

    const char *p = command;
    while (*p) {
        /* Skip quoted content */
        if (*p == '\'' || *p == '"') {
            p = skip_quoted(p, *p);
            continue;
        }

        /* Check dangerous patterns only outside quotes */
        if (*p == '$' && *(p + 1) == '(') return 1;
        if (*p == '`') return 1;
        if (*p == ';') return 1;

        if (*p == '>' && *(p + 1) == '>') { p += 2; continue; }
        if (*p == '>') return 1;

        if (*p == '&' && *(p + 1) == '&') { p += 2; continue; }
        if (*p == '|' && *(p + 1) == '|') { p += 2; continue; }

        if (strncmp(p, "system(", 7) == 0) return 1;
        if (strncmp(p, "exec(", 5) == 0) return 1;
        if (strncmp(p, "../", 3) == 0) return 1;
        if (strncmp(p, "..\\", 3) == 0) return 1;

        p++;
    }

    /* background & (trailing, outside quotes) */
    {
        size_t len = strlen(command);
        const char *trimmed = command + len;
        while (trimmed > command && (*(trimmed-1) == ' ' || *(trimmed-1) == '\t')) trimmed--;
        if (trimmed > command && *(trimmed-1) == '&') {
            if (trimmed - 1 == command || *(trimmed-2) != '&') {
                return 1;
            }
        }
    }

    return 0;
}

/**
 * Pipe segment check: validate every command in a pipeline.
 * Returns 0 if all segments pass, non-zero if any segment fails.
 */
static int pipe_check(const char *command) {
    if (!command || !strchr(command, '|')) return 0;

    /* Make a mutable copy for strtok */
    char *cmd_copy = strdup(command);
    if (!cmd_copy) return 1;

    char *segment = strtok(cmd_copy, "|");
    int ret = 0;

    while (segment != NULL) {
        /* Trim leading whitespace */
        while (*segment == ' ' || *segment == '\t') segment++;

        if (strlen(segment) > 0) {
            /* Check for tee in any pipe segment.
             * Manual extraction avoids nested strtok (not re-entrant). */
            {
                const char *p = segment;
                while (*p == ' ' || *p == '\t') p++;
                if (strncmp(p, "tee", 3) == 0
                    && (p[3] == ' ' || p[3] == '\t' || p[3] == '\0')) {
                    free(cmd_copy);
                    return 1;
                }
            }

            if (whitelist_check(segment) != 0) {
                ret = 1;
                break;
            }
            if (operator_check(segment) != 0) {
                ret = 1;
                break;
            }
        }
        segment = strtok(NULL, "|");
    }

    free(cmd_copy);
    return ret;
}

/* ---- platform-specific execution ---- */

#ifdef _WIN32

static ShellResult execute_command_windows(const char *command, const char *working_dir,
                                           int timeout_sec, size_t max_output_bytes,
                                           const char *shell_path) {
    ShellResult result;
    memset(&result, 0, sizeof(result));

    SECURITY_ATTRIBUTES sa;
    sa.nLength = sizeof(SECURITY_ATTRIBUTES);
    sa.bInheritHandle = TRUE;
    sa.lpSecurityDescriptor = NULL;

    HANDLE stdout_read = NULL, stdout_write = NULL;
    HANDLE stderr_read = NULL, stderr_write = NULL;

    if (!CreatePipe(&stdout_read, &stdout_write, &sa, 0) ||
        !CreatePipe(&stderr_read, &stderr_write, &sa, 0)) {
        result.success = 0;
        result.error = strdup("Failed to create pipes");
        result.error_code = 3;
        result.output = strdup("");
        return result;
    }

    SetHandleInformation(stdout_read, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(stderr_read, HANDLE_FLAG_INHERIT, 0);

    STARTUPINFOA si;
    PROCESS_INFORMATION pi;
    ZeroMemory(&si, sizeof(si));
    si.cb = sizeof(si);
    si.hStdOutput = stdout_write;
    si.hStdError = stderr_write;
    si.dwFlags |= STARTF_USESTDHANDLES;
    ZeroMemory(&pi, sizeof(pi));

    char cmd_line[8192];
    // Use detected shell: bash/sh/pwsh → -c,  cmd → /C
    const char *flag = (strstr(shell_path, "cmd") || strstr(shell_path, "CMD")) ? "/C" : "-c";
    if (strcmp(flag, "/C") == 0) {
        snprintf(cmd_line, sizeof(cmd_line), "%s /C %s", shell_path, command);
    } else {
        snprintf(cmd_line, sizeof(cmd_line), "\"%s\" %s \"%s\"", shell_path, flag, command);
    }

    BOOL proc_created = CreateProcessA(
        NULL, cmd_line, NULL, NULL, TRUE,
        CREATE_NO_WINDOW, NULL, working_dir,
        &si, &pi
    );

    CloseHandle(stdout_write);
    CloseHandle(stderr_write);

    if (!proc_created) {
        result.success = 0;
        result.error = strdup("Failed to create process");
        result.error_code = 3;
        result.output = strdup("");
        CloseHandle(stdout_read);
        CloseHandle(stderr_read);
        return result;
    }

    /* Read output - must read pipes BEFORE waiting, to avoid deadlock
     * when child output exceeds pipe buffer size (~4KB). */
    size_t out_cap = max_output_bytes + 1;
    char *out_buf = (char *)malloc(out_cap);
    size_t out_len = 0;
    int output_truncated = 0;

    char *err_buf = (char *)malloc(4096);
    size_t err_len = 0;
    size_t err_cap = 4096;

    DWORD start_tick = GetTickCount();
    DWORD timeout_ms = (DWORD)(timeout_sec * 1000);
    int stdout_eof = 0;
    int stderr_eof = 0;
    int timed_out = 0;

    /* Non-blocking read loop: read both pipes while child is running */
    while (!stdout_eof || !stderr_eof) {
        DWORD elapsed = GetTickCount() - start_tick;
        if (elapsed >= timeout_ms) {
            timed_out = 1;
            TerminateProcess(pi.hProcess, 1);
            WaitForSingleObject(pi.hProcess, 1000);
            break;
        }

        /* Check stdout - always consume data to prevent child from blocking */
        if (!stdout_eof) {
            DWORD avail = 0;
            if (PeekNamedPipe(stdout_read, NULL, 0, NULL, &avail, NULL) && avail > 0) {
                DWORD bytes_read;
                char tmp[4096];
                DWORD to_read = avail < sizeof(tmp) ? avail : sizeof(tmp);
                if (ReadFile(stdout_read, tmp, to_read, &bytes_read, NULL) && bytes_read > 0) {
                    if (out_len < max_output_bytes) {
                        size_t to_copy = bytes_read;
                        if (out_len + to_copy > max_output_bytes) {
                            to_copy = max_output_bytes - out_len;
                            output_truncated = 1;
                        }
                        memcpy(out_buf + out_len, tmp, to_copy);
                        out_len += to_copy;
                    } else {
                        output_truncated = 1;
                        /* Data consumed but discarded - keeps pipe flowing */
                    }
                }
            } else if (avail == 0) {
                /* Check if child has exited - if so, drain and mark EOF */
                DWORD wait_check = WaitForSingleObject(pi.hProcess, 0);
                if (wait_check == WAIT_OBJECT_0) {
                    /* Drain remaining data after child exit */
                    DWORD bytes_read;
                    char tmp[4096];
                    while (ReadFile(stdout_read, tmp, sizeof(tmp), &bytes_read, NULL) && bytes_read > 0) {
                        if (out_len < max_output_bytes) {
                            size_t to_copy = bytes_read;
                            if (out_len + to_copy > max_output_bytes) {
                                to_copy = max_output_bytes - out_len;
                                output_truncated = 1;
                            }
                            memcpy(out_buf + out_len, tmp, to_copy);
                            out_len += to_copy;
                        } else {
                            output_truncated = 1;
                        }
                    }
                    stdout_eof = 1;
                }
            }
        }

        /* Check stderr - always consume data */
        if (!stderr_eof) {
            DWORD avail = 0;
            if (PeekNamedPipe(stderr_read, NULL, 0, NULL, &avail, NULL) && avail > 0) {
                DWORD bytes_read;
                char tmp[4096];
                DWORD to_read = avail < sizeof(tmp) ? avail : sizeof(tmp);
                if (ReadFile(stderr_read, tmp, to_read, &bytes_read, NULL) && bytes_read > 0) {
                    if (err_len + bytes_read < err_cap) {
                        memcpy(err_buf + err_len, tmp, bytes_read);
                        err_len += bytes_read;
                    }
                    /* Always consume even if err_buf is full */
                }
            } else if (avail == 0) {
                DWORD wait_check = WaitForSingleObject(pi.hProcess, 0);
                if (wait_check == WAIT_OBJECT_0) {
                    DWORD bytes_read;
                    char tmp[4096];
                    while (ReadFile(stderr_read, tmp, sizeof(tmp), &bytes_read, NULL) && bytes_read > 0) {
                        if (err_len + bytes_read < err_cap) {
                            memcpy(err_buf + err_len, tmp, bytes_read);
                            err_len += bytes_read;
                        }
                    }
                    stderr_eof = 1;
                }
            }
        }

        /* Brief sleep to avoid busy-wait */
        if (!stdout_eof || !stderr_eof) {
            Sleep(1);
        }
    }

    if (timed_out) {
        result.success = 0;
        result.error_code = 4;
        result.error = strdup("Execution timeout");
        out_buf[out_len] = '\0';
        result.output = out_buf;
        result.output_len = out_len;
        result.output_truncated = output_truncated;
        result.exit_code = -1;
        free(err_buf);
        CloseHandle(stdout_read);
        CloseHandle(stderr_read);
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);
        return result;
    }

    /* Wait for process to fully exit */
    WaitForSingleObject(pi.hProcess, INFINITE);

    DWORD exit_code = 0;
    GetExitCodeProcess(pi.hProcess, &exit_code);

    out_buf[out_len] = '\0';
    err_buf[err_len] = '\0';

    result.success = (exit_code == 0) ? 1 : 0;
    result.output = out_buf;
    result.output_len = out_len;
    result.output_truncated = output_truncated;
    result.exit_code = (int)exit_code;
    result.error_code = 0;

    if (exit_code != 0 && err_len > 0) {
        result.error = strdup(err_buf);
    } else {
        result.error = strdup("");
    }

    free(err_buf);
    CloseHandle(stdout_read);
    CloseHandle(stderr_read);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    return result;
}

#else /* Unix */

static double time_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

static ShellResult execute_command_unix(const char *command, const char *working_dir,
                                        int timeout_sec, size_t max_output_bytes,
                                        const char *shell_path) {
    ShellResult result;
    memset(&result, 0, sizeof(result));

    int stdout_pipe[2], stderr_pipe[2];
    if (pipe(stdout_pipe) != 0 || pipe(stderr_pipe) != 0) {
        result.success = 0;
        result.error = strdup("Failed to create pipes");
        result.error_code = 3;
        result.output = strdup("");
        return result;
    }

    pid_t pid = fork();
    if (pid < 0) {
        result.success = 0;
        result.error = strdup("Fork failed");
        result.error_code = 3;
        result.output = strdup("");
        close(stdout_pipe[0]); close(stdout_pipe[1]);
        close(stderr_pipe[0]); close(stderr_pipe[1]);
        return result;
    }

    if (pid == 0) {
        /* Child */
        close(stdout_pipe[0]);
        close(stderr_pipe[0]);
        dup2(stdout_pipe[1], STDOUT_FILENO);
        dup2(stderr_pipe[1], STDERR_FILENO);
        close(stdout_pipe[1]);
        close(stderr_pipe[1]);

        if (chdir(working_dir) != 0) {
            _exit(127);
        }

        /* Clean environment */
        char *path = getenv("PATH");
        char *home = getenv("HOME");
        char *lang = getenv("LANG");
        char *path_copy = path ? strdup(path) : NULL;
        char *home_copy = home ? strdup(home) : NULL;
        char *lang_copy = lang ? strdup(lang) : NULL;

        extern char **environ;
        environ = calloc(4, sizeof(char*));

        if (path_copy) setenv("PATH", path_copy, 1);
        if (home_copy) setenv("HOME", home_copy, 1);
        if (lang_copy) setenv("LANG", lang_copy, 1);

        free(path_copy);
        free(home_copy);
        free(lang_copy);

        execl(shell_path, shell_path, "-c", command, (char *)NULL);
        _exit(127);
    }

    /* Parent */
    close(stdout_pipe[1]);
    close(stderr_pipe[1]);

    fcntl(stdout_pipe[0], F_SETFL, O_NONBLOCK);
    fcntl(stderr_pipe[0], F_SETFL, O_NONBLOCK);

    size_t out_cap = max_output_bytes + 1;
    char *out_buf = (char *)malloc(out_cap);
    size_t out_len = 0;
    int output_truncated = 0;

    char *err_buf = (char *)malloc(4096);
    size_t err_len = 0;
    size_t err_cap = 4096;

    double start = time_now();
    int child_exited = 0;
    int stdout_eof = 0;
    int stderr_eof = 0;

    while (!stdout_eof || !stderr_eof) {
        double elapsed = time_now() - start;
        double remaining = timeout_sec - elapsed;
        if (remaining <= 0) {
            kill(pid, SIGKILL);
            waitpid(pid, NULL, 0);
            result.success = 0;
            result.error_code = 4;
            result.error = strdup("Execution timeout");
            out_buf[out_len] = '\0';
            result.output = out_buf;
            result.output_len = out_len;
            result.output_truncated = output_truncated;
            result.exit_code = -1;
            free(err_buf);
            close(stdout_pipe[0]);
            close(stderr_pipe[0]);
            return result;
        }

        fd_set rfds;
        FD_ZERO(&rfds);
        int max_fd = 0;
        if (!stdout_eof) { FD_SET(stdout_pipe[0], &rfds); if (stdout_pipe[0] > max_fd) max_fd = stdout_pipe[0]; }
        if (!stderr_eof) { FD_SET(stderr_pipe[0], &rfds); if (stderr_pipe[0] > max_fd) max_fd = stderr_pipe[0]; }

        struct timeval tv;
        tv.tv_sec = (long)remaining;
        tv.tv_usec = (long)((remaining - (long)remaining) * 1e6);
        if (tv.tv_sec == 0 && tv.tv_usec == 0) tv.tv_usec = 100000;

        int ret = select(max_fd + 1, &rfds, NULL, NULL, &tv);
        if (ret < 0) {
            if (errno == EINTR) continue;
            break;
        }

        if (!stdout_eof && FD_ISSET(stdout_pipe[0], &rfds)) {
            char tmp[4096];
            ssize_t n = read(stdout_pipe[0], tmp, sizeof(tmp));
            if (n > 0) {
                if (out_len < max_output_bytes) {
                    size_t to_copy = (size_t)n;
                    if (out_len + to_copy > max_output_bytes) {
                        to_copy = max_output_bytes - out_len;
                        output_truncated = 1;
                    }
                    memcpy(out_buf + out_len, tmp, to_copy);
                    out_len += to_copy;
                } else {
                    output_truncated = 1;
                }
            } else if (n == 0) {
                stdout_eof = 1;
            }
        }

        if (!stderr_eof && FD_ISSET(stderr_pipe[0], &rfds)) {
            char tmp[4096];
            ssize_t n = read(stderr_pipe[0], tmp, sizeof(tmp));
            if (n > 0) {
                if (err_len + (size_t)n < err_cap) {
                    memcpy(err_buf + err_len, tmp, (size_t)n);
                    err_len += (size_t)n;
                }
            } else if (n == 0) {
                stderr_eof = 1;
            }
        }

        if (!child_exited) {
            int status;
            pid_t w = waitpid(pid, &status, WNOHANG);
            if (w > 0) {
                child_exited = 1;
                result.exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
            }
        }
    }

    if (!child_exited) {
        int status;
        waitpid(pid, &status, 0);
        result.exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
    }

    close(stdout_pipe[0]);
    close(stderr_pipe[0]);

    out_buf[out_len] = '\0';
    err_buf[err_len] = '\0';

    result.success = (result.exit_code == 0) ? 1 : 0;
    result.output = out_buf;
    result.output_len = out_len;
    result.output_truncated = output_truncated;
    result.error_code = 0;

    if (result.exit_code != 0 && err_len > 0) {
        result.error = strdup(err_buf);
    } else {
        result.error = strdup("");
    }

    free(err_buf);
    return result;
}

#endif /* _WIN32 */

ShellResult shell_execute(const char *command, const char *working_dir,
                          int timeout_sec, size_t max_output_bytes,
                          const char *shell_path) {
    ShellResult result;
    memset(&result, 0, sizeof(result));

    if (!command || !working_dir || !shell_path) {
        result.success = 0;
        result.error = strdup("Null argument");
        result.error_code = 3;
        result.output = strdup("");
        return result;
    }

    if (whitelist_check(command) != 0) {
        result.success = 0;
        result.error = strdup("Command not in whitelist");
        result.error_code = 1;
        result.output = strdup("");
        return result;
    }

    if (operator_check(command) != 0) {
        result.success = 0;
        result.error = strdup("Dangerous operator detected");
        result.error_code = 2;
        result.output = strdup("");
        return result;
    }

    if (pipe_check(command) != 0) {
        result.success = 0;
        result.error = strdup("Pipe segment contains non-whitelisted command or dangerous operator");
        result.error_code = 2;
        result.output = strdup("");
        return result;
    }

#ifdef _WIN32
    return execute_command_windows(command, working_dir, timeout_sec, max_output_bytes, shell_path);
#else
    return execute_command_unix(command, working_dir, timeout_sec, max_output_bytes, shell_path);
#endif
}

void shell_result_free(ShellResult *result) {
    if (result) {
        free(result->output);
        free(result->error);
        result->output = NULL;
        result->error = NULL;
    }
}
