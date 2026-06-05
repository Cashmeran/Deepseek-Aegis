import { useEffect, useState, useCallback, useRef } from "react";
import { useIPC } from "./hooks/useIPC";
import { useAppStore } from "./store/useAppStore";
import type { ClientEvent } from "./types";
import MDContent from "./render/markdown";

function App() {
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

  const [sideCollapsed, setSideCollapsed] = useState(false);
  const [showNewModal, setShowNewModal] = useState(false);
  const [cwd, setCwd] = useState("");
  const [projectName, setProjectName] = useState("");
  const [prompt, setPrompt] = useState("");

  const activeSession = activeSessionId ? sessions[activeSessionId] : null;
  const isRunning = activeSession?.status === "running";
  const list = Object.values(sessions).sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));

  const handleNewSession = useCallback(() => {
    const cfg = providerConfigs.deepseek;
    if (!cfg.apiKey) { alert("请配置 API Key"); return; }
    const name = projectName.trim() || (cwd ? cwd.split(/[\\/]/).pop() || cwd : "新项目");
    sendEvent({ type: "session.start", payload: { title: name, prompt: prompt || "Hello", cwd: cwd || undefined, provider: "deepseek", apiKey: cfg.apiKey, model: cfg.model } });
    setShowNewModal(false); setProjectName(""); setPrompt("");
  }, [sendEvent, projectName, prompt, cwd, providerConfigs]);

  const handleContinue = useCallback(() => {
    if (!prompt.trim() || !activeSessionId || isRunning) return;
    sendEvent({ type: "session.continue", payload: { sessionId: activeSessionId, prompt: prompt.trim() } });
    setPrompt(""); inputRef.current?.focus();
  }, [sendEvent, prompt, activeSessionId, isRunning]);

  const handleDelete = useCallback((id: string) => {
    sendEvent({ type: "session.delete", payload: { sessionId: id } });
  }, [sendEvent]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleContinue(); }
  }, [handleContinue]);

  const SideIcon = (p: { d: string }) => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d={p.d}/></svg>;

  return (
    <div className="app" data-side-collapsed={sideCollapsed ? "" : undefined} data-ctx-collapsed="" style={{ "--ctx-width": "0px", "--side-width": sideCollapsed ? "0px" : "244px" } as React.CSSProperties}>
      {/* title row */}
      <div style={{ gridArea: "title", display: "flex", alignItems: "center", padding: "0 16px", fontSize: 13, fontWeight: 600, color: "var(--fg)", background: "var(--bg-2)", borderBottom: "1px solid var(--border)" }}>
        Aegis Desktop {activeSession ? `· ${activeSession.title || activeSession.id}` : ""}
        <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--muted)", fontWeight: 400 }}>{connected ? "已连接" : "未连接"} · {providerConfigs.deepseek.model}</span>
      </div>
      {/* tabs row (unused, keep grid area) */}
      <div style={{ gridArea: "tabs", display: sideCollapsed ? "none" : "flex", alignItems: "center", gap: 6, padding: "0 16px", background: "var(--bg-2)", borderBottom: "1px solid var(--border)" }}>
        <button className="btn btn-primary btn-sm" onClick={() => setShowNewModal(true)}>+ 新建会话</button>
        <button className="btn btn-ghost btn-sm" onClick={() => setSideCollapsed(!sideCollapsed)}>{sideCollapsed ? "展开侧边栏" : "收起侧边栏"}</button>
      </div>

      {/* sidebar */}
      <div className="sidebar" style={{ gridArea: "side", display: sideCollapsed ? "none" : "flex", flexDirection: "column" }}>
        <div className="session-list" style={{ flex: 1, overflowY: "auto", padding: "4px 6px 12px" }}>
          {list.map(s => (
            <div key={s.id}
              style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px", borderRadius: 6, cursor: "pointer", fontSize: 13, color: s.id === activeSessionId ? "var(--accent)" : "var(--fg-2)", background: s.id === activeSessionId ? "var(--accent-soft)" : undefined, marginBottom: 1 }}
              onClick={() => setActiveSessionId(s.id)}>
              <span style={{ width: 6, height: 6, borderRadius: "50%", flexShrink: 0, background: s.status === "running" ? "var(--accent)" : s.status === "completed" ? "var(--success)" : s.status === "error" ? "var(--danger)" : "var(--muted)" }}/>
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.title || s.id}</span>
              <button className="icon-btn" style={{ width: 22, height: 22, flexShrink: 0 }} onClick={e => { e.stopPropagation(); handleDelete(s.id); }}><SideIcon d="M18 6L6 18M6 6l12 12"/></button>
            </div>
          ))}
          {list.length === 0 && <div className="tree-empty">暂无会话</div>}
        </div>
      </div>

      {/* main thread */}
      <div style={{ gridArea: "main", overflow: "hidden", display: "flex", flexDirection: "column" }}>
        <div className="thread" style={{ flex: 1, overflowY: "auto" }}>
          <div className="thread-inner" style={{ padding: "24px 32px" }}>
            {globalError && (
              <div style={{ padding: "8px 14px", background: "var(--danger-soft)", border: "1px solid var(--danger)", borderRadius: 8, color: "var(--danger)", fontSize: 13, marginBottom: 12 }}>
                {globalError} <button className="btn btn-ghost btn-sm" onClick={() => setGlobalError(null)}>Dismiss</button>
              </div>
            )}
            {activeSession ? (
              <>
                {activeSession.messages.map((msg: Record<string, unknown>, i: number) => {
                  const t = msg.type as string;
                  if (t === "user_prompt") return (
                    <div key={i} style={{ marginBottom: 20, display: "flex", justifyContent: "flex-end" }}>
                      <div style={{ maxWidth: "85%", background: "var(--accent-soft)", color: "var(--fg)", borderRadius: "var(--radius-lg)", borderBottomRightRadius: 4, padding: "12px 16px", fontSize: 14, lineHeight: 1.6 }}>
                        <MDContent text={String(msg.prompt ?? "")}/>
                      </div>
                    </div>
                  );
                  if (t === "assistant") return (
                    <div key={i} style={{ marginBottom: 20 }}>
                      <div style={{ background: "var(--card)", color: "var(--fg)", borderRadius: "var(--radius-lg)", borderBottomLeftRadius: 4, padding: "12px 16px", fontSize: 14, lineHeight: 1.6 }}>
                        <MDContent text={String(msg.text ?? "")}/>
                      </div>
                    </div>
                  );
                  if (t === "thinking") return (
                    <details key={i} style={{ marginBottom: 8, fontSize: 12, color: "var(--muted)" }}>
                      <summary style={{ cursor: "pointer", fontStyle: "italic", userSelect: "none" }}>Thinking...</summary>
                      <div style={{ marginTop: 4, padding: "8px 12px", background: "var(--card)", borderLeft: "2px solid var(--border)", borderRadius: "0 6px 6px 0", whiteSpace: "pre-wrap" }}>{msg.text as string}</div>
                    </details>
                  );
                  if (t === "tool_use") return (
                    <div key={i} style={{ marginBottom: 6 }}>
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "3px 10px", borderRadius: 9999, fontSize: 12, fontWeight: 600, background: (msg.status === "error" ? "var(--danger-soft)" : msg.status === "success" ? "var(--success-soft)" : "var(--card)"), color: (msg.status === "error" ? "var(--danger)" : msg.status === "success" ? "var(--success)" : "var(--muted)") }}>
                        {msg.name as string} {msg.elapsed_ms ? <span style={{ fontWeight: 400, opacity: 0.7 }}>{(msg.elapsed_ms as number)}ms</span> : null}
                      </span>
                      {msg.output ? <details style={{ marginTop: 2 }}><summary style={{ fontSize: 11, color: "var(--muted)", cursor: "pointer" }}>Output</summary><pre style={{ marginTop: 4, padding: "8px 12px", background: "var(--bg-2)", borderRadius: 6, fontSize: 12, whiteSpace: "pre-wrap", maxHeight: 200, overflow: "auto" }}>{msg.output as string}</pre></details> : null}
                    </div>
                  );
                  if (t === "usage") return (
                    <div key={i} style={{ textAlign: "center", padding: "12px 0" }}>
                      <span style={{ fontSize: 11, color: "var(--muted)", background: "var(--card)", padding: "4px 14px", borderRadius: 9999 }}>
                        {(msg as Record<string,unknown>).input_tokens as number} in / {(msg as Record<string,unknown>).output_tokens as number} out · ${Number((msg as Record<string,unknown>).cost).toFixed(4)}
                      </span>
                    </div>
                  );
                  return null;
                })}
                {isRunning && (
                  <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 0 12px", fontSize: 13, color: "var(--muted)" }}>
                    <span style={{ width: 8, height: 8, borderRadius: "50%", background: "var(--accent)", animation: "pulse 1.5s ease-in-out infinite" }}/>
                    Agent is working...
                  </div>
                )}
              </>
            ) : (
              <div className="splash" style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", height: "100%", textAlign: "center" }}>
                <div style={{ width: 56, height: 56, borderRadius: 16, background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 22, fontWeight: 800, color: "var(--accent)", marginBottom: 16 }}>ag</div>
                <div style={{ fontSize: 22, fontWeight: 700, marginBottom: 6, color: "var(--fg)" }}>Aegis Desktop</div>
                <div style={{ fontSize: 13, color: "var(--muted)", marginBottom: 24 }}>{connected ? "后端已连接" : "未连接"} · {providerConfigs.deepseek.apiKey ? providerConfigs.deepseek.model : "未配置 Key"}</div>
                <button className="btn btn-primary" onClick={() => setShowNewModal(true)}>+ 新建会话</button>
              </div>
            )}
          </div>
        </div>

        {/* composer */}
        {activeSession && (
          <div className="composer-wrap">
            <div className="composer-inner">
              <div className="composer">
                <textarea ref={inputRef} value={prompt} onChange={e => setPrompt(e.target.value)} onKeyDown={handleKeyDown}
                  placeholder={isRunning ? "Agent is working..." : "输入消息，Enter 发送"}
                  rows={1} disabled={isRunning}
                  onInput={e => { const el = e.currentTarget; el.style.height = "auto"; el.style.height = Math.min(el.scrollHeight, 200) + "px"; }}/>
                <div className="composer-foot">
                  <span className="cf-hint">Enter 发送 · Shift+Enter 换行</span>
                  <button className="cf-btn" onClick={handleContinue} disabled={!prompt.trim() || isRunning}>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* ctx panel (unused) */}
      <div style={{ gridArea: "ctx", display: "none" }}/>

      {/* status bar */}
      <div style={{ gridArea: "status", display: "flex", alignItems: "center", padding: "0 16px", fontSize: 11, color: "var(--muted)", background: "var(--bg-2)", borderTop: "1px solid var(--border)" }}>
        {activeSession ? `${activeSession.messages.length} messages · ${activeSession.status}` : "Ready"}
        <span style={{ marginLeft: "auto" }}>{providerConfigs.deepseek.apiKey ? `Key: ${providerConfigs.deepseek.model}` : "No API key"}</span>
      </div>

      {/* new session modal */}
      {showNewModal && (
        <div className="modal-overlay" onClick={() => setShowNewModal(false)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-title">新建会话</div>
            <div className="modal-field"><label>项目名称</label><input className="input" value={projectName} onChange={e => setProjectName(e.target.value)} placeholder="留空使用目录名"/></div>
            <div className="modal-field"><label>工作目录</label><input className="input" value={cwd} onChange={e => setCwd(e.target.value)} placeholder="留空使用当前目录"/></div>
            <div className="modal-field"><label>初始提示词 (可选)</label><textarea className="textarea" rows={3} value={prompt} onChange={e => setPrompt(e.target.value)} placeholder="告诉 Agent 要做什么..."/></div>
            <div className="modal-actions">
              <button className="btn btn-ghost" onClick={() => setShowNewModal(false)}>取消</button>
              <button className="btn btn-primary" onClick={handleNewSession}>创建</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
