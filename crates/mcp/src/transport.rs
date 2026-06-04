//! MCP transport layer — stdio subprocess + SSE HTTP transport.
//! Implements JSON-RPC 2.0 message framing over both transport types.

use crate::types::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
/// Result of sending a request and receiving a response.
type PendingRequest = std::sync::mpsc::Sender<Result<JsonRpcResponse, String>>;

// ═══════════════ Transport trait ═══════════════

/// Abstract MCP transport — sends JSON-RPC requests, receives responses.
/// Implementations: StdioTransport (subprocess), SseTransport (HTTP/SSE).
pub trait McpTransport: Send + Sync {
    fn send_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, String>;
    fn send_notification(&self, notification: &JsonRpcRequest) -> Result<(), String>;
    fn is_alive(&self) -> bool;
    fn close(&self);
    fn transport_type(&self) -> &str;
}

// ═══════════════ Stdio Transport ═══════════════

pub struct StdioTransport {
    stdin: Mutex<Box<dyn Write + Send>>,
    #[allow(dead_code)]
    #[allow(dead_code)]
    reader_thread: std::thread::JoinHandle<()>,
    alive: Arc<AtomicU64>,
    request_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
}

impl StdioTransport {
    pub fn new(command: &str, args: &[String], env: Option<&HashMap<String, String>>) -> Result<Self, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()); // stderr goes to parent

        if let Some(env_vars) = env {
            for (k, v) in env_vars {
                cmd.env(k, v);
            }
        }

        let mut child = cmd.spawn()
            .map_err(|e| format!("Failed to spawn MCP server '{}': {}", command, e))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| format!("No stdin for MCP server '{}'", command))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| format!("No stdout for MCP server '{}'", command))?;

        let alive = Arc::new(AtomicU64::new(1));
        let alive_clone = Arc::clone(&alive);
        let pending: Arc<Mutex<HashMap<u64, PendingRequest>>> = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        // Spawn reader thread for stdout JSON-RPC responses
        let reader_thread = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        alive_clone.store(1, Ordering::SeqCst);
                        let trimmed = line.trim();
                        if trimmed.is_empty() { continue; }
                        if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                            let mut map = pending_clone.lock().unwrap();
                            if let Some(sender) = map.remove(&response.id) {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        // Also check for notifications (no id field)
                        if let Ok(_notification) = serde_json::from_str::<JsonRpcNotification>(trimmed) {
                            // Handle notifications (e.g. tools/list_changed)
                        }
                    }
                    Err(_) => break,
                }
            }
            alive_clone.store(0, Ordering::SeqCst);
            // Reject all pending requests on disconnect
            let mut map = pending_clone.lock().unwrap();
            for (_, sender) in map.drain() {
                let _ = sender.send(Err("MCP server disconnected".into()));
            }
        });

        Ok(Self {
            stdin: Mutex::new(Box::new(stdin)),
            reader_thread,
            alive,
            request_id: AtomicU64::new(1),
            pending,
        })
    }

    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }
}

impl McpTransport for StdioTransport {
    fn send_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, String> {
        let id = request.id.unwrap_or_else(|| self.next_id());
        let mut req = request.clone();
        req.id = Some(id);

        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut map = self.pending.lock().unwrap();
            map.insert(id, tx);
        }

        // Serialize and send
        let json = serde_json::to_string(&req)
            .map_err(|e| format!("Serialize error: {}", e))?;

        {
            let mut stdin = self.stdin.lock().unwrap();
            writeln!(stdin, "{}", json)
                .map_err(|e| format!("Write error: {}", e))?;
            stdin.flush().map_err(|e| format!("Flush error: {}", e))?;
        }

        // Wait for response
        match rx.recv() {
            Ok(response) => response,
            Err(_) => Err("MCP server disconnected before responding".into()),
        }
    }

    fn send_notification(&self, notification: &JsonRpcRequest) -> Result<(), String> {
        let json = serde_json::to_string(notification)
            .map_err(|e| format!("Serialize error: {}", e))?;
        let mut stdin = self.stdin.lock().unwrap();
        writeln!(stdin, "{}", json).map_err(|e| format!("Write error: {}", e))?;
        stdin.flush().map_err(|e| format!("Flush error: {}", e))?;
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst) > 0
    }

    fn close(&self) {
        self.alive.store(0, Ordering::SeqCst);
    }

    fn transport_type(&self) -> &str { "stdio" }
}

// ═══════════════ SSE Transport (HTTP-based) ═══════════════

pub struct SseTransport {
    url: String,
    headers: HashMap<String, String>,
    http_client: reqwest::blocking::Client,
    alive: Arc<AtomicU64>,
    request_id: AtomicU64,
}

impl SseTransport {
    pub fn new(url: &str, headers: Option<&HashMap<String, String>>) -> Self {
        Self {
            url: url.to_string(),
            headers: headers.cloned().unwrap_or_default(),
            http_client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            alive: Arc::new(AtomicU64::new(1)),
            request_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    fn build_request(&self, _method: &str, body: &serde_json::Value) -> reqwest::blocking::RequestBuilder {
        let mut req = self.http_client
            .post(&self.url)
            .header("Content-Type", "application/json");
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        req.json(body)
    }
}

impl McpTransport for SseTransport {
    fn send_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, String> {
        let id = request.id.unwrap_or_else(|| self.next_id());
        let mut req = request.clone();
        req.id = Some(id);

        let body = serde_json::to_value(&req)
            .map_err(|e| format!("Serialize: {}", e))?;

        let response = self.build_request("mcp", &body)
            .send()
            .map_err(|e| format!("HTTP error: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}: {}", response.status(), response.text().unwrap_or_default()));
        }

        let resp: JsonRpcResponse = response.json()
            .map_err(|e| format!("JSON parse error: {}", e))?;

        Ok(resp)
    }

    fn send_notification(&self, notification: &JsonRpcRequest) -> Result<(), String> {
        let body = serde_json::to_value(notification)
            .map_err(|e| format!("Serialize: {}", e))?;
        self.build_request("mcp", &body)
            .send()
            .map_err(|e| format!("HTTP error: {}", e))?;
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst) > 0
    }

    fn close(&self) {
        self.alive.store(0, Ordering::SeqCst);
    }

    fn transport_type(&self) -> &str { "sse" }
}
