// ── 工具输出限制 ──
pub const DEFAULT_MAX_RESULT_SIZE_CHARS: usize = 50_000;
pub const MAX_TOOL_RESULT_TOKENS: usize = 100_000;
pub const MAX_TOOL_RESULTS_PER_MESSAGE_CHARS: usize = 200_000;

// ── Bash 工具 ──
pub const DEFAULT_BASH_TIMEOUT_MS: u64 = 120_000;
pub const SANDBOX_BASH_TIMEOUT_MS: u64 = 60_000;
pub const MAX_BASH_OUTPUT_CHARS: usize = 50_000;

// ── 文件工具 ──
pub const MAX_FILE_SIZE_BYTES: u64 = 262_144; // 256KB
pub const MAX_FILE_LINES: usize = 2000;
pub const MAX_READ_OUTPUT_TOKENS: usize = 25_000;

// ── Web Fetch ──
pub const FETCH_TIMEOUT_MS: u64 = 15_000;
pub const MAX_FETCH_CONTENT_BYTES: usize = 1_048_576; // 1MB

// ── Agent 循环 ──
pub const DEFAULT_MAX_TURNS: u32 = 100;
pub const MAX_PARALLEL_TOOLS: usize = 8;
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 120_000;

// ── 工具结果展示 (CC toolLimits.ts 对齐) ──
pub const TOOL_SUMMARY_MAX_LENGTH: usize = 50; // 工具调用摘要最大字符数

// ── 上下文 ──
pub const MAX_CONTEXT_TOKENS_AUTO_RATIO: f64 = 0.8; // 80% of model max
