import { useState, useEffect, type ReactElement } from "react";
import { I } from "../icons";

type SettingsFields = { apiKey: string; model: string; };

const AVAILABLE_MODELS = ["deepseek-v4-pro", "deepseek-v4-flash"];

type TabId = "general" | "context" | "mcp" | "logs";

export function SettingsModal({ onClose, apiKey, model, onSave, activeCwd }: {
  onClose: () => void; apiKey: string; model: string; onSave: (f: SettingsFields) => void; activeCwd?: string;
}): ReactElement {
  const [key, setKey] = useState(apiKey);
  const [showKey, setShowKey] = useState(false);
  const [mdl, setMdl] = useState(model);
  const [tab, setTab] = useState<TabId>("general");

  const [logDir, setLogDir] = useState("加载中…");
  useEffect(() => { window.__TAURI__?.core?.invoke<string>("get_log_dir").then(setLogDir).catch(() => setLogDir("不可用")); }, []);

  const [computerUseEnabled, setComputerUseEnabled] = useState(false);
  useEffect(() => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke<boolean>("get_computer_use_enabled", { cwd: activeCwd }).then(setComputerUseEnabled).catch(() => {});
  }, [activeCwd]);

  const toggleComputerUse = (v: boolean) => {
    setComputerUseEnabled(v);
    if (activeCwd) window.__TAURI__?.core?.invoke("set_computer_use_enabled", { cwd: activeCwd, enabled: v });
  };

  const [ctxMaxTurns, setCtxMaxTurns] = useState(25);
  const [ctxVerify, setCtxVerify] = useState(true);
  const [ctxMaxTokens, setCtxMaxTokens] = useState("0");
  useEffect(() => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke<{maxTurns:number;verifyBeforeOutput:boolean;maxContextTokens:number}>("get_compaction_config", { cwd: activeCwd })
      .then(c => { if (c) { setCtxMaxTurns(c.maxTurns); setCtxVerify(c.verifyBeforeOutput); setCtxMaxTokens(String(c.maxContextTokens||0)); } }).catch(() => {});
  }, [activeCwd]);

  const [mcpContent, setMcpContent] = useState("加载中…");
  useEffect(() => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke<string>("get_mcp_config", { cwd: activeCwd }).then(setMcpContent).catch(() => setMcpContent("{}"));
  }, [activeCwd]);

  const saveCtx = () => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke("save_compaction_config", { cwd: activeCwd, maxTurns: ctxMaxTurns, verify: ctxVerify, maxCtx: parseInt(ctxMaxTokens)||0 });
  };
  const saveMcp = () => { if (activeCwd) window.__TAURI__?.core?.invoke("save_mcp_config", { cwd: activeCwd, content: mcpContent }); };
  const openLogDir = () => { window.__TAURI__?.core?.invoke("open_log_dir").catch(() => {}); };
  const openMcpDir = () => { if (activeCwd) window.__TAURI__?.core?.invoke("open_mcp_config_dir", { cwd: activeCwd }).catch(() => {}); };

  const tabs: { id: TabId; label: string }[] = [
    { id: "general", label: "通用" }, { id: "context", label: "上下文" }, { id: "mcp", label: "MCP" }, { id: "logs", label: "日志" }
  ];

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="settings-modal settings-layout" onClick={e => e.stopPropagation()}>
        <div className="settings-nav">
          <div className="settings-nav-title">设置</div>
          {tabs.map(t => (
            <button key={t.id} onClick={() => setTab(t.id)}
              className="tab-btn" data-on={tab === t.id}
              style={{ textAlign: "left", width: "100%", borderRadius: "var(--radius-sm)", borderBottom: "none", marginBottom: 0 }}>
              {t.label}
            </button>
          ))}
          <div className="spacer" />
          <button className="btn-icon" onClick={onClose}><I.x /></button>
        </div>
        <div className="settings-content">
          {tab === "general" && <>
            <div className="settings-section">
              <h3>DeepSeek 配置</h3>
              <span className="settings-field-label">API Key</span>
              <div className="key-input-row">
                <input className="input" type={showKey ? "text" : "password"} value={key} onChange={e => setKey(e.target.value)} placeholder="sk-…" />
                <button onClick={() => setShowKey(!showKey)}>{showKey ? <I.eyeOff /> : <I.eye />}</button>
              </div>
              <span className="settings-field-label">Model</span>
              <select className="input" value={mdl} onChange={e => setMdl(e.target.value)}>
                {AVAILABLE_MODELS.map(m => <option key={m} value={m}>{m}</option>)}
              </select>
            </div>
            <div className="settings-section">
              <h3>计算机控制</h3>
              <div className="settings-toggle-row">
                <label>启用 Computer Use</label>
                <button className={`toggle ${computerUseEnabled ? "on" : "off"}`} onClick={() => toggleComputerUse(!computerUseEnabled)}>
                  <span className="toggle-thumb" />
                </button>
              </div>
              <div className="settings-hint">允许 Agent 控制鼠标键盘、截屏和操作桌面应用。默认关闭。开启后需重新开始对话生效。</div>
            </div>
            <div className="settings-section">
              <h3>快捷键</h3>
              <div className="text-sm text-secondary" style={{ lineHeight: 1.8 }}>
                <div><kbd className="kbd">Ctrl+K</kbd> 命令面板</div>
                <div><kbd className="kbd">Ctrl+B</kbd> 切换侧边栏</div>
                <div><kbd className="kbd">Ctrl+,</kbd> 设置</div>
                <div><kbd className="kbd">Enter</kbd> 发送 · <kbd className="kbd">Shift+Enter</kbd> 换行</div>
              </div>
            </div>
            <div style={{ paddingTop: 12, borderTop: "1px solid var(--border)" }}>
              <button className="btn btn-primary" onClick={() => { onSave({ apiKey: key, model: mdl }); onClose(); }}>保存并关闭</button>
            </div>
          </>}
          {tab === "context" && <>
            <div className="settings-section">
              <h3>上下文压缩</h3>
              <span className="settings-field-label">最大轮次</span>
              <input className="input" type="number" value={ctxMaxTurns} onChange={e => setCtxMaxTurns(Number(e.target.value))} min={5} max={100} />
              <div className="settings-hint" style={{ marginBottom: 10 }}>超过此轮次数自动触发上下文折叠。建议: 15-30</div>
              <span className="settings-field-label">上下文上限 (tokens, 0=自动)</span>
              <input className="input" type="number" value={ctxMaxTokens} onChange={e => setCtxMaxTokens(e.target.value)} min={0} />
              <div className="settings-hint" style={{ marginBottom: 10 }}>0 = 使用模型最大窗口的 80%。设置后覆盖自动检测</div>
              <div className="settings-toggle-row">
                <label>输出前验证</label>
                <button className={`toggle ${ctxVerify ? "on" : "off"}`} onClick={() => setCtxVerify(!ctxVerify)}>
                  <span className="toggle-thumb" />
                </button>
              </div>
              <div className="settings-hint">Agent 输出代码后自动跑 cargo check + cargo test 验证</div>
              <button className="btn btn-primary btn-sm" onClick={saveCtx} style={{ marginTop: 12 }}>保存上下文设置</button>
            </div>
          </>}
          {tab === "mcp" && <>
            <div className="settings-section">
              <div className="settings-section-title">
                <h3>MCP 服务器配置</h3>
                <span className="text-xs text-muted">.mcp.json</span>
              </div>
              <textarea className="textarea font-mono" value={mcpContent} onChange={e => setMcpContent(e.target.value)}
                style={{ minHeight: 200, fontSize: 12, lineHeight: 1.5 }} />
              <div className="flex-center gap-sm" style={{ marginTop: 8 }}>
                <button className="btn btn-primary btn-sm" onClick={saveMcp}>保存</button>
                <button className="btn btn-ghost btn-sm" onClick={openMcpDir}><I.folder /> 打开目录</button>
              </div>
            </div>
          </>}
          {tab === "logs" && <>
            <div className="settings-section">
              <h3>日志</h3>
              <span className="settings-field-label">日志目录</span>
              <div className="log-dir-display">
                <code>{logDir}</code>
                <button className="btn btn-ghost btn-sm" onClick={openLogDir}><I.folder /> 打开</button>
              </div>
              <div className="settings-hint">日志文件保存在此目录。出问题时查看最新日志排查。</div>
            </div>
          </>}
        </div>
      </div>
    </div>
  );
}
