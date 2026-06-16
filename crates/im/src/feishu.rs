//! Feishu/Lark adapter — WebSocket receive + REST API send.
//! Uses Feishu long-connection mode (no public IP needed).

use crate::{ImAdapter, ImMessage};
use serde::Deserialize;
use tokio::sync::mpsc;

pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    /// "feishu" or "lark" (for international users)
    pub brand: String,
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            app_secret: String::new(),
            brand: "feishu".into(),
        }
    }
}

pub struct FeishuAdapter {
    config: FeishuConfig,
    client: reqwest::Client,
}

impl FeishuAdapter {
    pub fn new(config: FeishuConfig) -> Self {
        Self { config, client: reqwest::Client::new() }
    }

    fn open_host(&self) -> &str {
        if self.config.brand == "lark" {
            "https://open.larksuite.com"
        } else {
            "https://open.feishu.cn"
        }
    }

    /// Obtain tenant_access_token via App ID + Secret.
    async fn get_token(&self) -> Result<String, String> {
        let url = format!("{}/open-apis/auth/v3/tenant_access_token/internal", self.open_host());
        let resp = self.client
            .post(&url)
            .json(&serde_json::json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("Token request failed: {}", e))?;

        #[derive(Deserialize)]
        struct TokenResp {
            code: i32,
            #[serde(default)]
            msg: String,
            #[serde(default)]
            tenant_access_token: String,
        }

        let body: TokenResp = resp.json().await.map_err(|e| format!("Token响应解析失败: {}", e))?;
        if body.code != 0 {
            return Err(format!("Token获取失败(AppID或Secret错误): code={} {}", body.code, body.msg));
        }
        if body.tenant_access_token.is_empty() {
            return Err("Token为空，请检查应用是否有效".into());
        }
        Ok(body.tenant_access_token)
    }

    /// Get dynamic WebSocket URL from Feishu.
    /// Calls POST /callback/ws/endpoint — returns (url, service_id, ping_interval_secs).
    async fn get_ws_endpoint(&self) -> Result<(String, i32, u64), String> {
        let url = format!("{}/callback/ws/endpoint", self.open_host());
        let resp = self.client
            .post(&url)
            .json(&serde_json::json!({
                "AppID": self.config.app_id,
                "AppSecret": self.config.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("WS endpoint request: {e}"))?;

        let status = resp.status();
        let raw = resp.text().await.map_err(|e| format!("WS endpoint body: {e}"))?;
        log::info!("WS endpoint HTTP {status}: {}", &raw[..raw.len().min(300)]);

        #[derive(Deserialize)]
        struct EndpointResp {
            code: i32,
            #[serde(default)] msg: String,
            #[serde(default)] data: EndpointData,
        }
        #[derive(Deserialize, Default)]
        struct EndpointData {
            #[serde(rename = "URL", default)] url: String,
            #[serde(rename = "ClientConfig", default)] client_config: ClientConfig,
        }
        #[derive(Deserialize, Default)]
        struct ClientConfig {
            #[serde(rename = "PingInterval", default)]
            ping_interval: u64,
            #[serde(rename = "ReconnectInterval", default)]
            _reconnect_interval: u64,
            #[serde(rename = "ReconnectNonce", default)]
            _reconnect_nonce: u64,
        }

        let body: EndpointResp = serde_json::from_str(&raw)
            .map_err(|e| format!("WS endpoint parse (HTTP {status}): {e}"))?;
        if body.code != 0 || body.data.url.is_empty() {
            return Err(format!("WS endpoint failed: code={} msg={}。请确认飞书应用已发布，事件订阅已开启长连接", body.code, body.msg));
        }

        let service_id = body.data.url
            .split("service_id=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        let ping_interval = if body.data.client_config.ping_interval > 0 {
            body.data.client_config.ping_interval
        } else {
            90
        };

        log::info!("WS endpoint: url={}, service_id={service_id}, ping={ping_interval}s", body.data.url);
        Ok((body.data.url, service_id, ping_interval))
    }

    /// Reply to a message via Feishu REST API.
    pub async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
        let token = self.get_token().await?;
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
            self.open_host()
        );
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": serde_json::json!({"text": text}).to_string(),
        });

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Send message: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Send failed ({}): {}", status.as_u16(), &text[..text.len().min(200)]));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ImAdapter for FeishuAdapter {
    fn platform(&self) -> &str { "feishu" }

    async fn send_reply(&self, chat_id: &str, text: &str) -> Result<(), String> {
        self.send_text(chat_id, text).await
    }

    async fn run(
        &self,
        msg_tx: mpsc::UnboundedSender<ImMessage>,
    ) -> Result<(), String> {
        let (ws_url, service_id, ping_interval) = self.get_ws_endpoint().await?;
        log::info!("Feishu WS connecting to {ws_url}");

        let (mut ws, _resp) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .map_err(|e| format!("WS connect failed: {e}"))?;

        log::info!("Feishu WS connected (ping={ping_interval}s, service={service_id})");

        let _ = msg_tx.send(ImMessage {
            chat_id: String::new(), text: String::new(),
            sender: "__connected__".into(), platform: "feishu".into(),
        });

        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let mut heartbeat = tokio::time::interval(
            std::time::Duration::from_secs(ping_interval)
        );

        loop {
            tokio::select! {
                msg = ws.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            log::debug!("Feishu WS text: {}", &text[..text.len().min(200)]);
                            if let Some(im_msg) = parse_feishu_event(&text) {
                                let _ = msg_tx.send(im_msg);
                            }
                        }
                        Some(Ok(Message::Binary(data))) => {
                            log::info!("Feishu WS recv binary: {} bytes", data.len());
                            match decode_frame(&data) {
                                Some(frame) => {
                                    let mtype = header_val(&frame.headers, "type").unwrap_or("?");
                                    log::info!("Feishu WS frame: method={} type={mtype} service={} payload={}b",
                                        frame.method, frame.service_id, frame.payload.len());
                                    if frame.method == METHOD_DATA {
                                        // Send ACK immediately with seq_id/log_id so Feishu matches it
                                        let msg_id = header_val(&frame.headers, "message_id").unwrap_or("");
                                        let sum = header_val(&frame.headers, "sum").unwrap_or("");
                                        let seq = header_val(&frame.headers, "seq").unwrap_or("");
                                        let biz_rt = header_val(&frame.headers, "biz_rt").unwrap_or("");
                                        let ack = encode_frame(
                                            frame.seq_id, frame.log_id, service_id, METHOD_DATA,
                                            &[
                                                ("type", "event"),
                                                ("message_id", msg_id),
                                                ("sum", sum),
                                                ("seq", seq),
                                                ("biz_rt", biz_rt),
                                            ],
                                            br#"{"code":200}"#,
                                        );
                                        let _ = ws.send(Message::Binary(ack.into())).await;

                                        if let Some(im_msg) = parse_feishu_binary(&data) {
                                            log::info!("Feishu IM msg: {} @ {}: {}", im_msg.sender, im_msg.chat_id, im_msg.text);
                                            let _ = msg_tx.send(im_msg);
                                        }
                                    } else if frame.method == METHOD_CONTROL && mtype == "ping" {
                                        let pong = build_pong_frame(service_id);
                                        let _ = ws.send(Message::Binary(pong.into())).await;
                                        log::debug!("Feishu WS pong sent");
                                    }
                                }
                                None => {
                                    log::warn!("Feishu WS failed to decode frame: {}b", data.len());
                                }
                            }
                        }
                        Some(Ok(Message::Ping(_))) => {
                            let _ = ws.send(Message::Pong(vec![])).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            log::warn!("Feishu WS closed");
                            break;
                        }
                        Some(Err(e)) => {
                            log::warn!("Feishu WS error: {e}");
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                _ = heartbeat.tick() => {
                    let ping = build_ping_frame(service_id);
                    if ws.send(Message::Binary(ping.into())).await.is_err() {
                        log::warn!("Feishu WS ping send failed");
                        break;
                    }
                }
            }
        }

        Err("Feishu WS connection ended".into())
    }
}

// ── Minimal protobuf frame decoder ──────────────────────────────────
// Feishu WS uses protobuf-encoded Frame messages (pbbp2.proto).
// Frame fields: seq_id(1), log_id(2), service(3), method(4),
//               headers(5), payload_encoding(6), payload_type(7), payload(8)
// Header fields: key(1), value(2)

const METHOD_CONTROL: i32 = 0;
const METHOD_DATA: i32 = 1;

struct PbFrame {
    seq_id: u64,
    log_id: u64,
    method: i32,
    service_id: i32,
    headers: Vec<(String, String)>,
    payload: Vec<u8>,
}

/// Minimal protobuf decoder — only handles varint and length-delimited fields.
fn decode_frame(data: &[u8]) -> Option<PbFrame> {
    let mut seq_id = 0u64;
    let mut log_id = 0u64;
    let mut method = 0i32;
    let mut service_id = 0i32;
    let mut headers = Vec::new();
    let mut payload = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        let (tag, adv) = read_varint(&data[pos..])?;
        pos += adv;
        let field_num = (tag >> 3) as u32;
        let wire_type = (tag & 0x07) as u8;

        match (field_num, wire_type) {
            (1, 0) => { let (v, a) = read_varint(&data[pos..])?; seq_id = v; pos += a; }
            (2, 0) => { let (v, a) = read_varint(&data[pos..])?; log_id = v; pos += a; }
            (3, 0) => { let (v, a) = read_varint(&data[pos..])?; service_id = v as i32; pos += a; }
            (4, 0) => { let (v, a) = read_varint(&data[pos..])?; method = v as i32; pos += a; }
            (5, 2) => {
                let (len, a) = read_varint(&data[pos..])?;
                pos += a;
                let end = pos + len as usize;
                // Parse repeated Header messages
                let mut hdr_pos = pos;
                while hdr_pos < end && hdr_pos < data.len() {
                    if let Some((k, v, adv)) = decode_header(&data[hdr_pos..end]) {
                        headers.push((k, v));
                        hdr_pos += adv;
                    } else { break; }
                }
                pos = end.min(data.len());
            }
            (6, 2) => { let (len, a) = read_varint(&data[pos..])?; pos += a + len as usize; } // payload_encoding
            (7, 2) => { let (len, a) = read_varint(&data[pos..])?; pos += a + len as usize; } // payload_type
            (8, 2) => {
                let (len, a) = read_varint(&data[pos..])?;
                pos += a;
                let end = (pos + len as usize).min(data.len());
                payload = data[pos..end].to_vec();
                pos = end;
            }
            (9, 0) => { let (_v, a) = read_varint(&data[pos..])?; pos += a; } // LogIDNew (varint)
            (9, 2) => { let (len, a) = read_varint(&data[pos..])?; pos += a + len as usize; } // LogIDNew (string)
            (_, 0) => { let (_v, a) = read_varint(&data[pos..])?; pos += a; } // unknown varint
            (_, 2) => { let (len, a) = read_varint(&data[pos..])?; pos += a + len as usize; } // unknown bytes
            _ => { break; } // unknown wire type — stop
        }
    }
    Some(PbFrame { seq_id, log_id, method, service_id, headers, payload })
}

fn decode_header(data: &[u8]) -> Option<(String, String, usize)> {
    let mut key = String::new();
    let mut val = String::new();
    let mut pos = 0;
    while pos < data.len() {
        let (tag, adv) = read_varint(&data[pos..])?;
        pos += adv;
        match (tag >> 3, (tag & 0x07) as u8) {
            (1, 2) => {
                let (len, a) = read_varint(&data[pos..])?;
                pos += a;
                key = String::from_utf8_lossy(&data[pos..pos + len as usize]).to_string();
                pos += len as usize;
            }
            (2, 2) => {
                let (len, a) = read_varint(&data[pos..])?;
                pos += a;
                val = String::from_utf8_lossy(&data[pos..pos + len as usize]).to_string();
                pos += len as usize;
            }
            _ => { break; }
        }
    }
    if key.is_empty() { None } else { Some((key, val, pos)) }
}

fn read_varint(data: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0;
    for (i, &byte) in data.iter().enumerate() {
        if i >= 10 { return None; } // max varint length
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
    }
    None
}

fn header_val<'a>(headers: &'a [(String, String)], key: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Build a proper Feishu protobuf ping frame (CONTROL, type=ping).
fn build_ping_frame(service_id: i32) -> Vec<u8> {
    encode_frame(0, 0, service_id, METHOD_CONTROL, &[("type", "ping")], &[])
}

/// Build a pong response frame.
fn build_pong_frame(service_id: i32) -> Vec<u8> {
    encode_frame(0, 0, service_id, METHOD_CONTROL, &[("type", "pong")], &[])
}

/// Encode a protobuf Frame message.
fn encode_frame(seq_id: u64, log_id: u64, service_id: i32, method: i32, headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    // field 1 (seq_id): varint
    if seq_id != 0 { write_varint(&mut buf, 1 << 3 | 0); write_varint(&mut buf, seq_id); }
    // field 2 (log_id): varint
    if log_id != 0 { write_varint(&mut buf, 2 << 3 | 0); write_varint(&mut buf, log_id); }
    // field 3 (service): varint
    write_varint(&mut buf, 3 << 3 | 0);
    write_varint(&mut buf, service_id as u64);
    // field 4 (method): varint
    write_varint(&mut buf, 4 << 3 | 0);
    write_varint(&mut buf, method as u64);
    // field 5 (headers): length-delimited repeated
    let mut hdr_buf = Vec::new();
    for (k, v) in headers {
        let h = encode_header(k, v);
        hdr_buf.extend_from_slice(&h);
    }
    write_varint(&mut buf, 5 << 3 | 2);
    write_varint(&mut buf, hdr_buf.len() as u64);
    buf.extend_from_slice(&hdr_buf);
    // field 8 (payload): length-delimited
    if !payload.is_empty() {
        write_varint(&mut buf, 8 << 3 | 2);
        write_varint(&mut buf, payload.len() as u64);
        buf.extend_from_slice(payload);
    }
    buf
}

fn encode_header(key: &str, value: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    // field 1 (key): length-delimited
    write_varint(&mut buf, 1 << 3 | 2);
    write_varint(&mut buf, key.len() as u64);
    buf.extend_from_slice(key.as_bytes());
    // field 2 (value): length-delimited
    write_varint(&mut buf, 2 << 3 | 2);
    write_varint(&mut buf, value.len() as u64);
    buf.extend_from_slice(value.as_bytes());
    buf
}

fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 { break; }
    }
}

/// Decode a binary protobuf frame and extract ImMessage from DATA events.
fn parse_feishu_binary(data: &[u8]) -> Option<ImMessage> {
    let frame = decode_frame(data)?;
    if frame.method != METHOD_DATA { return None; }

    let msg_type = header_val(&frame.headers, "type").unwrap_or("");
    log::debug!("Feishu frame: method={}, type={msg_type}, payload={}b", frame.method, frame.payload.len());

    if msg_type == "pong" { return None; }
    if msg_type != "event" && msg_type != "message" { return None; }

    // Payload is JSON for event frames
    let json_str = std::str::from_utf8(&frame.payload).ok()?;
    log::debug!("Feishu event: {}", &json_str[..json_str.len().min(300)]);
    parse_feishu_event(json_str)
}

/// Parse a Feishu WebSocket event into a normalized ImMessage.
/// Handles both v1.0 and v2.0 event formats.
///
/// v1.0: {"type":"event","event":{"type":"...","message":{"chat_id":"...","content":"..."}}}
/// v2.0: {"schema":"2.0","header":{"event_type":"..."},"event":{"message":{...},"sender":{...}}}
fn parse_feishu_event(raw: &str) -> Option<ImMessage> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;

    // Detect v2.0 by schema field
    let is_v2 = v.get("schema").and_then(|s| s.as_str()) == Some("2.0");

    let (msg, sender_node) = if is_v2 {
        let event = v.get("event")?;
        let msg = event.get("message")?;
        let sender = event.get("sender");
        (msg, sender)
    } else {
        let event = v.get("event")?;
        let msg = event.get("message")?;
        let sender = msg.get("sender");
        (msg, sender)
    };

    let chat_id = msg.get("chat_id")?.as_str()?.to_string();
    let content_str = msg.get("content")?.as_str()?;

    // Message content is a JSON string: {"text":"..."}
    let text = serde_json::from_str::<serde_json::Value>(content_str)
        .ok()
        .and_then(|c| c.get("text")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| content_str.to_string());

    let sender = sender_node
        .and_then(|s| s.get("sender_id"))
        .and_then(|s| s.get("open_id"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            sender_node
                .and_then(|s| s.get("open_id"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("unknown")
        .to_string();

    Some(ImMessage { chat_id, text, sender, platform: "feishu".into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end Feishu WS test — needs FEISHU_APP_ID + FEISHU_APP_SECRET.
    /// Requires a PUBLISHED app with long-connection enabled and `im.message.receive_v1` subscribed.
    /// Run: `cargo test -p aegis-im -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn test_feishu_ws_connection() {
        let app_id = std::env::var("FEISHU_APP_ID").expect("FEISHU_APP_ID not set");
        let app_secret = std::env::var("FEISHU_APP_SECRET").expect("FEISHU_APP_SECRET not set");

        let config = FeishuConfig { app_id, app_secret, brand: "feishu".into() };
        let adapter = FeishuAdapter::new(config);

        // Step 1: get token (validates credentials)
        let token = adapter.get_token().await.expect("get_token failed");
        println!("[OK] Token: {}… — credentials valid", &token[..8.min(token.len())]);

        // Step 2: get dynamic WS URL from endpoint
        let (ws_url, service_id, ping_interval) = adapter.get_ws_endpoint().await
            .expect("get_ws_endpoint failed. Ensure app is published with long connection enabled");
        println!("[OK] WS URL: {ws_url}");
        println!("[OK] service_id={service_id}, ping={ping_interval}s");

        // Step 3: connect WS — just verify connection works
        let (_ws, resp) = tokio_tungstenite::connect_async(&ws_url).await
            .expect("WS connect failed");
        println!("[OK] WS connected! HTTP {}", resp.status());
    }
}
