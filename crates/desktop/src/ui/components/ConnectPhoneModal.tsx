import { useState, useEffect, type ReactElement } from "react";
import { I } from "../icons";

type PlatformId = "feishu" | "wechat" | "telegram" | "dingtalk";

const PLATFORMS = [
  { id: "feishu" as const, name: "飞书", icon: I.zap, available: true },
  { id: "wechat" as const, name: "微信", icon: I.at, available: false },
  { id: "telegram" as const, name: "Telegram", icon: I.send, available: false },
  { id: "dingtalk" as const, name: "钉钉", icon: I.activity, available: false },
];

type ImStatus = {
  platform: string;
  configured: boolean;
  enabled: boolean;
  connected: boolean;
  app_id: string;
  app_secret: string;
  project_dir: string | null;
  last_error: string | null;
};

export function ConnectPhoneModal({ onClose }: { onClose: () => void }): ReactElement {
  const [platform, setPlatform] = useState<PlatformId>("feishu");
  const [appId, setAppId] = useState("");
  const [appSecret, setAppSecret] = useState("");
  const [showSecret, setShowSecret] = useState(false);
  const [busy, setBusy] = useState(false);
  const [enabled, setEnabled] = useState(false);
  const [status, setStatus] = useState<ImStatus | null>(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [showSteps, setShowSteps] = useState(false);

  const showMsg = (ok: boolean, text: string) => setMessage({ ok, text });

  useEffect(() => {
    window.__TAURI__?.core?.invoke<ImStatus>("get_im_config")
      .then(s => {
        if (s) {
          setStatus(s);
          setEnabled(s.enabled);
          setAppId(s.app_id);
          setAppSecret(s.app_secret);
          setPlatform(s.platform as PlatformId);
        }
      })
      .catch(() => {});
  }, []);

  // Poll connection status while modal is open
  useEffect(() => {
    const t = setInterval(() => {
      window.__TAURI__?.core?.invoke<ImStatus>("get_im_config")
        .then(s => { if (s) setStatus(s); })
        .catch(() => {});
    }, 3000);
    return () => clearInterval(t);
  }, []);

  const handleTest = async () => {
    if (!appId.trim() || !appSecret.trim()) { showMsg(false, "请填写 App ID 和 App Secret"); return; }
    setBusy(true);
    try {
      const msg = await window.__TAURI__?.core?.invoke<string>("test_im_connection", {
        appId: appId.trim(), appSecret: appSecret,
      });
      showMsg(true, msg);
    } catch (e: any) { showMsg(false, String(e)); }
    setBusy(false);
  };

  const handleToggle = async () => {
    const newEnabled = !enabled;
    setEnabled(newEnabled);
    setBusy(true);
    try {
      const result = await window.__TAURI__?.core?.invoke<ImStatus>("save_im_config", {
        platform, appId: appId.trim(), appSecret: appSecret, enabled: newEnabled,
      });
      if (result) setStatus(result);
    } catch (e: any) { showMsg(false, String(e)); }
    setBusy(false);
  };

  const handleSave = async () => {
    if (!appId.trim() || !appSecret.trim()) { showMsg(false, "请填写 App ID 和 App Secret"); return; }
    setBusy(true);
    try {
      const result = await window.__TAURI__?.core?.invoke<ImStatus>("save_im_config", {
        platform, appId: appId.trim(), appSecret: appSecret, enabled: true,
      });
      if (result) setStatus(result);
      showMsg(true, "已保存，正在连接飞书…");
    } catch (e: any) { showMsg(false, String(e)); }
    setBusy(false);
  };

  const handleDisconnect = async () => {
    setBusy(true);
    try {
      const result = await window.__TAURI__?.core?.invoke<ImStatus>("save_im_config", {
        platform, appId: "", appSecret: "",
      });
      if (result) setStatus(result);
      setAppId(""); setAppSecret("");
      showMsg(true, "已断开");
    } catch (e: any) { showMsg(false, String(e)); }
    setBusy(false);
  };

  const activePlatform = PLATFORMS.find(p => p.id === platform)!;
  const isConnected = status?.connected === true;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="phone-modal" onClick={e => e.stopPropagation()}>

        <div className="phone-nav">
          <div className="phone-nav-title">连接手机</div>
          {PLATFORMS.map(p => (
            <button key={p.id} onClick={() => { setPlatform(p.id); setMessage(null); }}
              className={`phone-platform-btn ${platform === p.id ? "active" : ""}`} disabled={!p.available}>
              <p.icon /><span>{p.name}</span>
              {!p.available && <span className="phone-badge">即将</span>}
            </button>
          ))}
          <div className="spacer" />
          <button className="btn-icon" onClick={onClose}><I.x /></button>
        </div>

        <div className="phone-content">
          {!activePlatform.available && (
            <div className="phone-placeholder">
              <I.smartphone /><h3>{activePlatform.name} 桥接</h3><p className="text-secondary">即将推出</p>
            </div>
          )}

          {activePlatform.available && (<>
            <div className="phone-section">
              <div className="phone-section-header">
                <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                  <button className={`toggle ${enabled ? "on" : "off"}`} onClick={handleToggle}>
                    <span className="toggle-thumb" />
                  </button>
                  <h3>飞书 IM 桥接</h3>
                </div>
                <button className="phone-steps-toggle" onClick={() => setShowSteps(!showSteps)}>
                  {showSteps ? <I.chevronLeft /> : <I.info />} 如何配置？
                </button>
              </div>
              {showSteps && (
                <div className="phone-steps">
                  <div className="phone-step"><span>1</span> 前往 <a href="#" onClick={e => { e.preventDefault(); window.open("https://open.feishu.cn", "_blank"); }}>飞书开放平台</a> 创建<strong>企业自建应用</strong>（系统自动添加机器人能力）</div>
                  <div className="phone-step"><span>2</span> 确认已开通权限：<code>im:message.p2p_msg:readonly</code> + <code>im:message:send_as_bot</code> + <code>im:message.group_at_msg:readonly</code></div>
                  <div className="phone-step"><span>3</span> 事件订阅 → 开启 <strong>长连接</strong> → 订阅事件 <code>im.message.receive_v1</code></div>
                  <div className="phone-step"><span>4</span> 创建版本并<strong>发布</strong>（审批通过后生效）</div>
                  <div className="phone-step"><span>5</span> 飞书客户端搜索应用名 → 发消息测试。复制 <strong>App ID</strong> 和 <strong>App Secret</strong> 填入下方</div>
                </div>
              )}
            </div>

            <div className="phone-section">
              <label className="phone-label">App ID</label>
              <input className="input font-mono" value={appId}
                onChange={e => { setAppId(e.target.value); setMessage(null); }}
                placeholder="cli_xxxxxxxxxxxxxxxx" style={{ fontSize: 13 }} />

              <label className="phone-label" style={{ marginTop: 12 }}>App Secret</label>
              <div className="key-input-row">
                <input className="input font-mono"
                  type={showSecret ? "text" : "password"}
                  value={appSecret}
                  onChange={e => { setAppSecret(e.target.value); setMessage(null); }}
                  placeholder="输入 App Secret" style={{ fontSize: 13 }} />
                <button onClick={() => setShowSecret(!showSecret)}>
                  {showSecret ? <I.eyeOff /> : <I.eye />}
                </button>
              </div>
            </div>

            <div className="phone-section" style={{ paddingTop: 0, paddingBottom: 0 }}>
              {message && (
                <div className={`phone-msg ${message.ok ? "ok" : "err"}`}>
                  <span className="phone-msg-dot" />{message.text}
                </div>
              )}
              {!message && status?.last_error && (
                <div className="phone-msg err" style={{ wordBreak: "break-all" }}>
                  <span className="phone-msg-dot" />{status.last_error}
                </div>
              )}
              {!message && !status?.last_error && isConnected && (
                <div className="phone-msg ok">
                  <span className="phone-msg-dot" />
                  已连接{status?.project_dir ? ` · 项目: ${status.project_dir.split(/[/\\]/).pop()}` : ""}
                </div>
              )}
              {!message && !status?.last_error && status?.configured && !isConnected && (
                <div className="phone-msg"><span className="phone-msg-dot" />等待 WebSocket 连接…（确认飞书应用已发布并审核通过）</div>
              )}
            </div>

            <div className="phone-section" style={{ paddingTop: 8 }}>
              <div className="phone-actions">
                <button className="btn btn-ghost btn-sm" onClick={handleTest} disabled={busy}>
                  {busy ? "…" : "测试连接"}
                </button>
                {isConnected ? (<>
                  <button className="btn btn-primary btn-sm" onClick={handleSave} disabled={busy}>重连</button>
                  <button className="btn btn-danger btn-sm" onClick={handleDisconnect} disabled={busy}>断开</button>
                </>) : (
                  <button className="btn btn-primary btn-sm" onClick={handleSave} disabled={busy}>保存并连接</button>
                )}
              </div>
            </div>
          </>)}
        </div>
      </div>
    </div>
  );
}
