use std::collections::HashMap;

/// LSP 诊断集成配置。
#[derive(Debug, Clone)]
pub struct LspConfig {
    /// 编辑后等待 LSP 索引的时间 (ms)
    pub poll_after_edit_ms: u64,
    /// 每个文件最多注入的诊断条数
    pub max_diagnostics_per_file: u32,
    /// 是否注入 warning (默认仅注入 error)
    pub include_warnings: bool,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            poll_after_edit_ms: 5_000,
            max_diagnostics_per_file: 20,
            include_warnings: false,
        }
    }
}

impl LspConfig {
    pub fn from_agent_config(config: &crate::types::config::AgentConfig) -> Self {
        Self {
            poll_after_edit_ms: config.lsp_poll_after_edit_ms,
            max_diagnostics_per_file: config.lsp_max_diagnostics_per_file as u32,
            include_warnings: config.lsp_include_warnings,
        }
    }
}

/// 支持的语言 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    Rust,
    Go,
    Python,
    TypeScript,
    Cpp,
}

impl LanguageId {
    /// 从文件扩展名推断语言。
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "go" => Some(Self::Go),
            "py" | "pyi" => Some(Self::Python),
            "ts" | "tsx" | "js" | "jsx" => Some(Self::TypeScript),
            "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" => Some(Self::Cpp),
            _ => None,
        }
    }

    /// 返回推荐的 LSP 服务器命令。
    pub fn server_command(&self) -> &[&str] {
        match self {
            Self::Rust => &["rust-analyzer"],
            Self::Go => &["gopls"],
            Self::Python => &["pyright-langserver", "--stdio"],
            Self::TypeScript => &["typescript-language-server", "--stdio"],
            Self::Cpp => &["clangd"],
        }
    }

    /// 语言名称 (用于日志)。
    pub fn name(&self) -> &str {
        match self {
            Self::Rust => "Rust",
            Self::Go => "Go",
            Self::Python => "Python",
            Self::TypeScript => "TypeScript",
            Self::Cpp => "C/C++",
        }
    }
}

/// LSP 诊断管理器。
///
/// 管理最多 5 个 LSP 服务器进程 (每种语言一个)。
/// 非阻塞: LSP 崩溃 → 跳过本轮诊断 → 下次编辑自动重试 → Agent 正常继续。
///
/// Phase 2: 仅定义配置和语言映射，实际 LSP 通信在 Phase 3 通过 `lsp-types` crate 实现。
pub struct LspManager {
    /// 配置
    config: LspConfig,
    /// 最近一次诊断结果缓存 (文件路径 → 诊断文本)
    diagnostics_cache: HashMap<String, Vec<String>>,
}

impl LspManager {
    pub fn new(config: LspConfig) -> Self {
        Self {
            config,
            diagnostics_cache: HashMap::new(),
        }
    }

    /// 为指定文件检测语言。
    pub fn detect_language(file_path: &str) -> Option<LanguageId> {
        std::path::Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .and_then(LanguageId::from_extension)
    }

    /// 获取缓存的诊断结果，格式化为系统消息。
    /// Phase 2: 返回缓存内容。Phase 3: 实际查询 LSP 服务器。
    pub fn get_diagnostics_for_file(&self, file_path: &str) -> Option<String> {
        self.diagnostics_cache.get(file_path).map(|diags| {
            if diags.is_empty() {
                String::new()
            } else {
                let mut output = format!("## LSP Diagnostics for {}\n", file_path);
                for d in diags {
                    output.push_str(d);
                    output.push('\n');
                }
                output
            }
        })
    }

    /// 注入诊断结果作为合成 SystemMessage。
    /// 格式: "LSP: src/main.rs:42 error[E0596]: cannot borrow..."
    pub fn format_diagnostic_injection(diagnostics: &str) -> String {
        if diagnostics.is_empty() {
            return String::new();
        }
        format!(
            "[LSP Diagnostics — fix blocking errors before proceeding]\n{}",
            diagnostics
        )
    }

    /// 记录诊断结果到缓存。
    pub fn cache_diagnostics(&mut self, file_path: &str, diagnostics: Vec<String>) {
        self.diagnostics_cache
            .insert(file_path.to_string(), diagnostics);
    }

    /// 清除文件缓存 (文件被删除或重命名时)。
    pub fn clear_cache_for(&mut self, file_path: &str) {
        self.diagnostics_cache.remove(file_path);
    }

    pub fn config(&self) -> &LspConfig {
        &self.config
    }

    /// Run cargo check and collect diagnostics (Phase 3 — lightweight LSP alternative).
    /// Returns structured diagnostics string for injection into system prompt.
    pub fn cargo_check_diagnostics(working_dir: &std::path::Path) -> Option<String> {
        let output = std::process::Command::new("cargo")
            .args(["check", "--message-format=json"])
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        for line in stdout.lines() {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                let level = msg.get("level").and_then(|v| v.as_str()).unwrap_or("");
                let message = msg.get("message").and_then(|v| v.get("rendered")).and_then(|v| v.as_str())
                    .or_else(|| msg.get("message").and_then(|v| v.as_str()))
                    .unwrap_or("");
                if message.is_empty() { continue; }
                match level {
                    "error" => errors.push(message.to_string()),
                    "warning" => warnings.push(message.to_string()),
                    _ => {}
                }
            }
        }

        if errors.is_empty() && warnings.is_empty() {
            return None;
        }

        let mut result = String::from("## Cargo Check Diagnostics\n");
        for e in &errors {
            result.push_str(&format!("- ERROR: {}\n", e.lines().next().unwrap_or(e)));
        }
        for w in warnings.iter().take(5) {
            result.push_str(&format!("- warning: {}\n", w.lines().next().unwrap_or(w)));
        }
        if warnings.len() > 5 {
            result.push_str(&format!("- ... and {} more warnings\n", warnings.len() - 5));
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        assert_eq!(LanguageId::from_extension("rs"), Some(LanguageId::Rust));
        assert_eq!(LanguageId::from_extension("go"), Some(LanguageId::Go));
        assert_eq!(LanguageId::from_extension("py"), Some(LanguageId::Python));
        assert_eq!(LanguageId::from_extension("ts"), Some(LanguageId::TypeScript));
        assert_eq!(LanguageId::from_extension("cpp"), Some(LanguageId::Cpp));
        assert_eq!(LanguageId::from_extension("txt"), None);
    }

    #[test]
    fn test_detect_language_from_path() {
        assert_eq!(
            LspManager::detect_language("src/main.rs"),
            Some(LanguageId::Rust)
        );
        assert_eq!(
            LspManager::detect_language("app.tsx"),
            Some(LanguageId::TypeScript)
        );
        assert_eq!(LspManager::detect_language("README.md"), None);
    }

    #[test]
    fn test_server_commands() {
        assert_eq!(LanguageId::Rust.server_command(), &["rust-analyzer"]);
        assert_eq!(LanguageId::Go.server_command(), &["gopls"]);
    }

    #[test]
    fn test_diagnostic_formatting() {
        let diags = "src/main.rs:10 error[E0308]: mismatched types";
        let formatted = LspManager::format_diagnostic_injection(diags);
        assert!(formatted.contains("[LSP Diagnostics"));
        assert!(formatted.contains("E0308"));
    }

    #[test]
    fn test_empty_diagnostics() {
        assert_eq!(LspManager::format_diagnostic_injection(""), "");
    }

    #[test]
    fn test_cache_and_retrieve() {
        let mut mgr = LspManager::new(LspConfig::default());
        mgr.cache_diagnostics(
            "src/main.rs",
            vec!["error: unused variable".into()],
        );

        let result = mgr.get_diagnostics_for_file("src/main.rs");
        assert!(result.unwrap().contains("unused variable"));
        assert!(mgr.get_diagnostics_for_file("nonexistent.rs").is_none());
    }
}
