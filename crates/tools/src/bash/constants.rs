/// Bash 工具默认超时 (ms)。2 分钟足够编译+运行测试, 防止无限循环。
pub const DEFAULT_BASH_TIMEOUT_MS: u64 = 120_000;
/// 沙箱内 Bash 超时 (ms)。
pub const SANDBOX_BASH_TIMEOUT_MS: u64 = 60_000;
/// 最大输出字符数。超过此值转为文件引用。
pub const MAX_BASH_OUTPUT_CHARS: usize = 50_000;
/// 危险命令列表 (需要用户明确确认)。
pub const DESTRUCTIVE_COMMANDS: &[&str] = &[
    "rm -rf", "rm -r", "sudo", "chmod 777", "chown",
    "mkfs", "dd if=", ":(){ :|:& };:",
    "> /dev/sda", "git push --force", "git reset --hard",
];
/// 禁止写入的敏感路径。
pub const PROTECTED_PATHS: &[&str] = &[
    "/etc", "/boot", "/sys", "/proc", "~/.ssh", "~/.gnupg",
];
