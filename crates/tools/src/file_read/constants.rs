/// 最大读取行数 (参考值)。
pub const MAX_LINES: usize = 2000;
/// 最大文件大小 (字节), 256KB (参考值)。
pub const MAX_FILE_SIZE_BYTES: u64 = 262_144;
/// 禁止读取的文件 (安全检查)。
pub const PROTECTED_READ_FILES: &[&str] = &[
    ".env", ".env.local", ".gitconfig", ".mcp.json", ".claude.json",
    "id_rsa", "id_ed25519", "id_ecdsa",
];
