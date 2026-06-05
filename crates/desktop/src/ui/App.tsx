import { useEffect, useState, useCallback, useRef } from "react";
import { useIPC } from "./hooks/useIPC";
import { useAppStore } from "./store/useAppStore";
import type { ClientEvent } from "./types";
import MDContent from "./render/markdown";

function App() {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const initConfig = useAppStore((s) => s.initConfig);
  useEffect(() => { initConfig(); }, []);

  const handleServerEvent = useAppStore((s) => s.handleServerEvent);
  const { connected, sendEvent } = useIPC(handleServerEvent);

  const sessions = useAppStore((s) => s.sessions);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const setActiveSessionId = useAppStore((s) => s.setActiveSessionId);
  const providerConfigs = useAppStore((s) => s.providerConfigs);
  const globalError = useAppStore((s) => s.globalError);
  const setGlobalError = useAppStore((s) => s.setGlobalError);

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [showNewModal, setShowNewModal] = useState(false);
  const [cwd, setCwd] = useState("");
  const [projectName, setProjectName] = useState("");
  const [prompt, setPrompt] = useState("");

  const activeSession = activeSessionId ? sessions[activeSessionId] : null;
  const isRunning = activeSession?.status === "running";
  const sessionList = Object.values(sessions).sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeSession?.messages]);

  const handleNewSession = useCallback(() => {
    const cfg = providerConfigs.deepseek;
    if (!cfg.apiKey) { alert("请先在设置中配置 API Key"); return; }
    const name = projectName.trim() || (cwd ? cwd.split(/[\\/]/).pop() || cwd : "新项目");
    sendEvent({ type: "session.start", payload: { title: name, prompt: prompt || "Hello", cwd: cwd || undefined, provider: "deepseek", apiKey: cfg.apiKey, model: cfg.model } });
    setShowNewModal(false);
    setProjectName(""); setPrompt("");
  }, [sendEvent, projectName, prompt, cwd, providerConfigs]);

  const handleContinue = useCallback(() => {
    if (!prompt.trim() || !activeSessionId || isRunning) return;
    sendEvent({ type: "session.continue", payload: { sessionId: activeSessionId, prompt: prompt.trim() } });
    setPrompt("");
    inputRef.current?.focus();
  }, [sendEvent, prompt, activeSessionId, isRunning]);

  const handleDeleteSession = useCallback((id: string) => {
    sendEvent({ type: "session.delete", payload: { sessionId: id } });
  }, [sendEvent]);

  return (
    <div className="app-layout">
      {/* ── Sidebar ── */}
      <aside className={`sidebar ${sidebarCollapsed ? "sidebar-collapsed" : ""}`}>
        <div className="sidebar-header">
          {!sidebarCollapsed && <span className="sidebar-logo">Aegis</span>}
          <div className="sidebar-actions">
            <button className="btn-icon" title="新建项目" onClick={() => setShowNewModal(true)}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 5v14M5 12h14"/></svg>
            </button>
            <button className="btn-icon" title={sidebarCollapsed ? "展开" : "收起"} onClick={() => setSidebarCollapsed(!sidebarCollapsed)}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                {sidebarCollapsed ? <path d="M13 5l7 7-7 7"/> : <path d="M11 5l-7 7 7 7"/>}
              </svg>
            </button>
          </div>
        </div>

        {!sidebarCollapsed && (
          <>
            <div className="sidebar-sessions">
              {sessionList.map(s => (
                <div key={s.id} className={`session-item ${s.id === activeSessionId ? "active" : ""}`}
                  onClick={() => setActiveSessionId(s.id)}>
                  <span className={`session-item-status ${s.status}`} />
                  <span className="session-item-title">{s.title || s.id}</span>
                  <button className="btn-icon btn-sm" onClick={e => { e.stopPropagation(); handleDeleteSession(s.id); }}
                    title="删除">
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
                  </button>
                </div>
              ))}
              {sessionList.length === 0 && (
                <div style={{ fontSize: 12, color: 'var(--muted)', textAlign: 'center', padding: 24 }}>
                  暂无项目<br/>点击 + 创建
                </div>
              )}
            </div>
            <div className="sidebar-footer">
              {connected ? "已连接" : "未连接"} · {providerConfigs.deepseek.apiKey ? providerConfigs.deepseek.model : "未配置 Key"}
            </div>
          </>
        )}
      </aside>

      {/* ── Main ── */}
      <div className="main-area">
        <div className="messages-area">
          {globalError && (
            <div className="error-banner">
              <span>{globalError}</span>
              <button className="btn btn-ghost btn-sm" onClick={() => setGlobalError(null)}>Dismiss</button>
            </div>
          )}

          {activeSession ? (
            <div>
              {activeSession.messages.map((msg: Record<string, unknown>, i: number) => {
                const t = msg.type as string;
                if (t === "user_prompt") return (
                  <div key={i} className="msg-row msg-user">
                    <div className="msg-label">You</div>
                    <div className="msg-bubble"><MDContent text={String(msg.prompt ?? "")} /></div>
                  </div>
                );
                if (t === "assistant") return (
                  <div key={i} className="msg-row msg-assistant">
                    <div className="msg-bubble"><MDContent text={String(msg.text ?? "")} /></div>
                  </div>
                );
                if (t === "thinking") return (
                  <details key={i} className="msg-thinking">
                    <summary>Thinking...</summary>
                    <div className="msg-thinking-content">{msg.text as string}</div>
                  </details>
                );
                if (t === "tool_use") return (
                  <div key={i} className="msg-tool">
                    <span className={`msg-tool-badge ${msg.status === "error" ? "error" : msg.status === "success" ? "success" : "pending"}`}>
                      {msg.name as string}
                      {msg.elapsed_ms ? <span style={{ opacity: 0.6, fontWeight: 400 }}>{(msg.elapsed_ms as number)}ms</span> : null}
                    </span>
                    {msg.output ? (
                      <details className="msg-tool-output">
                        <summary>Output</summary>
                        <pre>{msg.output as string}</pre>
                      </details>
                    ) : null}
                  </div>
                );
                if (t === "usage") return (
                  <div key={i} className="msg-usage">
                    <span>{(msg as Record<string,unknown>).input_tokens as number} in / {(msg as Record<string,unknown>).output_tokens as number} out · ${Number((msg as Record<string,unknown>).cost).toFixed(4)}</span>
                  </div>
                );
                return null;
              })}
              {isRunning && <div className="running-indicator"><div className="dot-pulse" /> Agent is working...</div>}
              <div ref={messagesEndRef} />
            </div>
          ) : (
            <div className="empty-state">
              <div className="empty-logo">ag</div>
              <div className="empty-title">Aegis Desktop</div>
              <div className="empty-desc">
                {connected ? "后端已连接" : "后端未连接"} · {providerConfigs.deepseek.apiKey ? `模型: ${providerConfigs.deepseek.model}` : "未配置 API Key"}
              </div>
              <button className="btn btn-primary" onClick={() => setShowNewModal(true)}>+ 新建项目</button>
            </div>
          )}
        </div>

        {activeSession && (
          <div className="input-area">
            <div className="input-row">
              <textarea
                ref={inputRef}
                className="chat-input"
                value={prompt}
                onChange={e => setPrompt(e.target.value)}
                onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleContinue(); } }}
                placeholder={isRunning ? "Agent 正在工作中..." : "输入消息，Enter 发送，Shift+Enter 换行"}
                rows={1}
                disabled={isRunning}
                onInput={e => {
                  const el = e.currentTarget;
                  el.style.height = "auto";
                  el.style.height = Math.min(el.scrollHeight, 200) + "px";
                }}
              />
              <button className="btn btn-primary" onClick={handleContinue} disabled={!prompt.trim() || isRunning}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
              </button>
            </div>
          </div>
        )}
      </div>

      {/* ── New Project Modal ── */}
      {showNewModal && (
        <div className="modal-overlay" onClick={() => setShowNewModal(false)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-title">新建项目</div>
            <div className="modal-field">
              <label>项目名称</label>
              <input className="input" value={projectName} onChange={e => setProjectName(e.target.value)} placeholder="留空使用目录名" />
            </div>
            <div className="modal-field">
              <label>工作目录</label>
              <input className="input" value={cwd} onChange={e => setCwd(e.target.value)} placeholder="留空使用当前目录" />
            </div>
            <div className="modal-field">
              <label>初始提示词（可选）</label>
              <textarea className="textarea" rows={3} value={prompt} onChange={e => setPrompt(e.target.value)} placeholder="直接告诉 Agent 要做什么..." />
            </div>
            <div className="modal-actions">
              <button className="btn btn-ghost" onClick={() => setShowNewModal(false)}>取消</button>
              <button className="btn btn-primary" onClick={handleNewSession}>创建项目</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
