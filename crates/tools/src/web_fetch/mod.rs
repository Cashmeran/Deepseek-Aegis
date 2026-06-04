pub mod constants;

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use constants::{FETCH_TIMEOUT_MS, MAX_CONTENT_BYTES};
use std::sync::Arc;

/// Parsed HTTP fetch result with MIME info.
struct FetchResult {
    content: String,
    mime_type: String,
    content_length: usize,
    is_html: bool,
    is_json: bool,
}

/// Web Fetch 工具——获取 URL 内容，MIME 类型感知 + HTML→text 转换。
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self { Self }

    /// 解析 Content-Type header → (mime_type, charset).
    fn parse_content_type(header: &str) -> (String, String) {
        let parts: Vec<&str> = header.split(';').collect();
        let mime = parts.first().map(|s| s.trim().to_lowercase()).unwrap_or_default();
        let mut charset = String::from("utf-8");
        for part in &parts[1..] {
            let kv: Vec<&str> = part.splitn(2, '=').collect();
            if kv.len() == 2 && kv[0].trim().eq_ignore_ascii_case("charset") {
                charset = kv[1].trim().trim_matches('"').trim_matches('\'').to_lowercase();
            }
        }
        (mime, charset)
    }

    /// 将 HTML 转换为可读纯文本。
    fn html_to_text(html: &str) -> String {
        let mut result = String::with_capacity(html.len() / 3);
        let mut in_tag = false;
        let mut in_script = false;
        let mut in_style = false;
        let mut tag_name = String::new();
        let mut skip_until_close = String::new();
        let mut last_was_newline = false;
        let mut last_was_space = false;

        let bytes = html.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            let ch = bytes[i];

            if in_tag {
                if ch == b'>' {
                    in_tag = false;
                    // Block-level elements → newline
                    let tag = tag_name.trim().to_lowercase();
                    if matches!(tag.as_str(), "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "hr" | "section" | "article" | "header" | "footer" | "nav" | "main" | "aside") {
                        if !last_was_newline { result.push('\n'); last_was_newline = true; last_was_space = false; }
                    }
                    if tag == "script" { in_script = true; skip_until_close = "script".into(); }
                    if tag == "style" { in_style = true; skip_until_close = "style".into(); }
                    tag_name.clear();
                } else {
                    tag_name.push(ch as char);
                }
                i += 1;
                continue;
            }

            if in_script || in_style {
                // Look for </script> or </style>
                if ch == b'<' {
                    let remaining = &bytes[i..];
                    let remaining_str = String::from_utf8_lossy(remaining).to_lowercase();
                    if remaining_str.starts_with(&format!("</{}", skip_until_close)) {
                        // Find the closing >
                        if let Some(end) = remaining.iter().position(|&c| c == b'>') {
                            i += end + 1;
                            in_script = false;
                            in_style = false;
                            skip_until_close.clear();
                            continue;
                        }
                    }
                }
                i += 1;
                continue;
            }

            if ch == b'<' {
                in_tag = true;
                tag_name.clear();
                i += 1;
                continue;
            }

            // Decode common HTML entities
            if ch == b'&' {
                let remaining = &bytes[i..];
                let remaining_str = String::from_utf8_lossy(remaining);
                if remaining_str.starts_with("&amp;") { result.push('&'); i += 5; last_was_space = false; last_was_newline = false; continue; }
                if remaining_str.starts_with("&lt;") { result.push('<'); i += 4; last_was_space = false; last_was_newline = false; continue; }
                if remaining_str.starts_with("&gt;") { result.push('>'); i += 4; last_was_space = false; last_was_newline = false; continue; }
                if remaining_str.starts_with("&quot;") { result.push('"'); i += 6; last_was_space = false; last_was_newline = false; continue; }
                if remaining_str.starts_with("&#39;") { result.push('\''); i += 5; last_was_space = false; last_was_newline = false; continue; }
                if remaining_str.starts_with("&nbsp;") { result.push(' '); i += 6; last_was_space = true; last_was_newline = false; continue; }
                if remaining_str.starts_with("&#") {
                    if let Some(end) = remaining_str.find(';') {
                        let entity = &remaining_str[2..end];
                        if let Ok(codepoint) = entity.parse::<u32>() {
                            if let Some(c) = char::from_u32(codepoint) {
                                result.push(c);
                                i += end + 1;
                                last_was_space = false; last_was_newline = false;
                                continue;
                            }
                        }
                    }
                }
            }

            // Collapse whitespace
            if ch.is_ascii_whitespace() {
                if !last_was_space && !last_was_newline {
                    result.push(' ');
                    last_was_space = true;
                }
                if ch == b'\n' || ch == b'\r' {
                    if !last_was_newline { result.push('\n'); last_was_newline = true; last_was_space = false; }
                }
                i += 1;
                continue;
            }

            result.push(ch as char);
            last_was_space = false;
            last_was_newline = false;
            i += 1;
        }

        // Clean up: remove leading/trailing whitespace per line, collapse blank lines
        let cleaned: Vec<&str> = result.lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        cleaned.join("\n")
    }

    /// Pretty-print JSON if possible, otherwise return as-is.
    fn format_json(content: &str) -> String {
        match serde_json::from_str::<serde_json::Value>(content) {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| content.to_string()),
            Err(_) => content.to_string(),
        }
    }

    async fn fetch_url(url: &str) -> AgentResult<FetchResult> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(FETCH_TIMEOUT_MS))
            .user_agent("aegis/0.1 (web_fetch tool)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| AgentError::ToolExecutionError {
                tool: "web_fetch".into(),
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        let response = client.get(url).send().await.map_err(|e| {
            AgentError::ToolExecutionError {
                tool: "web_fetch".into(),
                message: format!("HTTP request failed: {}", e),
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(AgentError::ApiError {
                status: status.as_u16(),
                body: format!("HTTP {} from {}", status, url),
            });
        }

        // Parse Content-Type header
        let content_type = response.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain");
        let (mime_type, charset) = Self::parse_content_type(content_type);

        let is_html = mime_type.contains("html");
        let is_json = mime_type.contains("json") || mime_type.contains("javascript");

        // Read response body
        let bytes = response.bytes().await.map_err(|e| AgentError::ToolExecutionError {
            tool: "web_fetch".into(),
            message: format!("Failed to read response body: {}", e),
        })?;

        if bytes.len() > MAX_CONTENT_BYTES {
            return Err(AgentError::FileTooLarge {
                size_bytes: bytes.len() as u64,
                limit_bytes: MAX_CONTENT_BYTES as u64,
            });
        }

        let content_length = bytes.len();

        // Decode based on charset
        let raw = if charset == "utf-16" || charset == "utf-16le" {
            let u16s: Vec<u16> = bytes.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&u16s)
        } else if charset == "utf-16be" {
            let u16s: Vec<u16> = bytes.chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&u16s)
        } else {
            String::from_utf8_lossy(&bytes).to_string()
        };

        // Post-process based on MIME type
        let content = if is_html {
            Self::html_to_text(&raw)
        } else if is_json {
            Self::format_json(&raw)
        } else {
            raw
        };

        Ok(FetchResult {
            content,
            mime_type,
            content_length,
            is_html,
            is_json,
        })
    }
}

impl Default for WebFetchTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for WebFetchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_fetch".into(),
            description: "Fetches content from a URL, auto-detects MIME type, converts HTML to text".into(),
            prompt: "Use web_fetch to retrieve content from a URL.\n\
                     - Works with HTTP and HTTPS URLs\n\
                     - Auto-detects MIME type from Content-Type header\n\
                     - HTML pages are automatically converted to readable plain text\n\
                     - JSON responses are pretty-printed\n\
                     - Response limited to 1MB\n\
                     - Timeout after 15 seconds".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch content from"
                    },
                    "raw": {
                        "type": "boolean",
                        "description": "Return raw content without HTML→text conversion (default: false)"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for WebFetchTool {
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        _ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let url = tool_use.input.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let raw = tool_use.input.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);

        if url.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "web_fetch".into(), errors: "url is required".into(),
            });
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(AgentError::ToolValidationError {
                tool: "web_fetch".into(),
                errors: "Only http:// and https:// URLs are supported".into(),
            });
        }

        let start = std::time::Instant::now();
        let result = Self::fetch_url(url).await?;
        let elapsed = start.elapsed().as_millis() as u64;

        // Format header with MIME info
        let header = format!(
            "[{} {} ({} bytes, {}ms)]\n\n",
            result.mime_type,
            if result.is_html { "(HTML→text)" } else if result.is_json { "(JSON)" } else { "" },
            result.content_length,
            elapsed,
        );

        let final_content = if raw {
            format!("{}{}", header, result.content)
        } else {
            format!("{}{}", header, result.content)
        };

        let content_block = if final_content.len() > 50_000 {
            ContentBlock::FileReference {
                path: ".agent/web_fetch_result.txt".into(),
                preview: final_content.chars().take(500).collect(),
                total_bytes: final_content.len() as u64,
            }
        } else {
            ContentBlock::Text { text: final_content }
        };

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![content_block],
            elapsed_ms: elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_type() {
        let (mime, charset) = WebFetchTool::parse_content_type("text/html; charset=utf-8");
        assert_eq!(mime, "text/html");
        assert_eq!(charset, "utf-8");
    }

    #[test]
    fn test_parse_content_type_json() {
        let (mime, charset) = WebFetchTool::parse_content_type("application/json");
        assert_eq!(mime, "application/json");
        assert_eq!(charset, "utf-8");
    }

    #[test]
    fn test_parse_content_type_with_quotes() {
        let (mime, charset) = WebFetchTool::parse_content_type("text/html; charset=\"UTF-16\"");
        assert_eq!(mime, "text/html");
        assert_eq!(charset, "utf-16");
    }

    #[test]
    fn test_html_to_text_strips_tags() {
        let html = "<html><body><p>Hello world</p></body></html>";
        let text = WebFetchTool::html_to_text(html);
        assert!(text.contains("Hello world"));
        assert!(!text.contains("<html>"));
        assert!(!text.contains("<p>"));
    }

    #[test]
    fn test_html_to_text_handles_entities() {
        let html = "<p>a &lt; b &amp;&amp; c &gt; d</p>";
        let text = WebFetchTool::html_to_text(html);
        assert!(text.contains("a < b && c > d"));
    }

    #[test]
    fn test_html_to_text_skips_script() {
        let html = "<html><script>console.log('evil')</script><p>safe</p></html>";
        let text = WebFetchTool::html_to_text(html);
        assert!(!text.contains("console.log"));
        assert!(text.contains("safe"));
    }

    #[test]
    fn test_html_to_text_skips_style() {
        let html = "<html><style>body { color: red; }</style><p>text</p></html>";
        let text = WebFetchTool::html_to_text(html);
        assert!(!text.contains("color: red"));
        assert!(text.contains("text"));
    }

    #[test]
    fn test_html_to_text_block_elements() {
        let html = "<div>line1</div><div>line2</div>";
        let text = WebFetchTool::html_to_text(html);
        assert!(text.contains("line1"));
        assert!(text.contains("line2"));
    }

    #[test]
    fn test_format_json() {
        let raw = "{\"key\":\"value\",\"num\":42}";
        let formatted = WebFetchTool::format_json(raw);
        assert!(formatted.contains("\"key\""));
        assert!(formatted.contains("\"value\""));
        assert!(formatted.contains("42"));
        // Pretty-print has newlines
        assert!(formatted.contains('\n'));
    }

    #[test]
    fn test_format_json_invalid() {
        let raw = "not json at all";
        let formatted = WebFetchTool::format_json(raw);
        assert_eq!(formatted, "not json at all");
    }

    #[tokio::test]
    async fn test_rejects_non_http_url() {
        let tool = Arc::new(WebFetchTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(), env: Default::default(),
            sandbox_enabled: false, sandbox: None,
            timeout_ms: 10_000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "t".into(), name: "web_fetch".into(),
            input: serde_json::json!({"url": "file:///etc/passwd"}),
        };
        let err = tool.execute(&tool_use, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("http"));
    }

    #[tokio::test]
    async fn test_requires_url() {
        let tool = Arc::new(WebFetchTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(), env: Default::default(),
            sandbox_enabled: false, sandbox: None,
            timeout_ms: 10_000, ask_user_cb: Default::default(), progress_tx: None };
        let tool_use = ToolUse {
            id: "t".into(), name: "web_fetch".into(),
            input: serde_json::json!({"url": ""}),
        };
        let err = tool.execute(&tool_use, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("required"));
    }
}
