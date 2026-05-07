fn main() {
    cc::Build::new()
        .file("csrc/shell_executor.c")
        .include("csrc")
        .compile("shell_executor");
}
