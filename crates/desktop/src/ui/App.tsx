import { useEffect, useState, useCallback } from "react";
import { useIPC } from "./hooks/useIPC";
import { useAppStore } from "./store/useAppStore";
import type { ServerEvent, ClientEvent } from "./types";
// import { Sidebar } from "./components/Sidebar";  // TODO: fix Sidebar crash

function App() {
  const initConfig = useAppStore((s) => s.initConfig);
  useEffect(() => { initConfig(); }, []);

  const handleServerEvent = useAppStore((s) => s.handleServerEvent);
  const { connected, sendEvent } = useIPC(handleServerEvent);

  const sessions = useAppStore((s) => s.sessions);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const setActiveSessionId = useAppStore((s) => s.setActiveSessionId);
  const providerConfigs = useAppStore((s) => s.providerConfigs);
  const globalError = useAppStore((s) => s.globalError);

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [showNewModal, setShowNewModal] = useState(false);
  const [cwd, setCwd] = useState("");
  const [prompt, setPrompt] = useState("");

  const activeSession = activeSessionId ? sessions[activeSessionId] : null;

  const handleNewSession = useCallback(() => {
    const cfg = providerConfigs.deepseek;
    if (!cfg.apiKey) { alert("请先配置 API Key"); return; }
    sendEvent({
      type: "session.start",
      payload: { title: prompt.slice(0, 30) || "新会话", prompt, cwd: cwd || undefined, provider: "deepseek", apiKey: cfg.apiKey, model: cfg.model },
    });
    setShowNewModal(false);
    setPrompt("");
  }, [sendEvent, prompt, cwd, providerConfigs]);

  const handleContinue = useCallback(() => {
    if (!prompt.trim() || !activeSessionId) return;
    sendEvent({ type: "session.continue", payload: { sessionId: activeSessionId, prompt: prompt.trim() } });
    setPrompt("");
  }, [sendEvent, prompt, activeSessionId]);

  const handleDeleteSession = useCallback((id: string) => {
    sendEvent({ type: "session.delete", payload: { sessionId: id } });
  }, [sendEvent]);

  return (
    <div style={{ display: 'flex', height: '100vh', background: '#0d1117', color: '#c9d1d9', fontFamily: 'system-ui, sans-serif' }}>
      {/* Inline sidebar while Sidebar component is broken */}
      <div style={{ width: sidebarCollapsed ? 40 : 220, borderRight: '1px solid #21262d', padding: 12, transition: 'width 0.2s', overflow: 'hidden', flexShrink: 0 }}>
        <div style={{ display: 'flex', gap: 6, marginBottom: 12 }}>
          <button onClick={() => setSidebarCollapsed(!sidebarCollapsed)} style={{ padding: '4px 8px', background: 'transparent', border: 'none', color: '#8b949e', cursor: 'pointer', fontSize: 13 }}>{sidebarCollapsed ? '>>' : '<<'}</button>
          {!sidebarCollapsed && <button onClick={() => setShowNewModal(true)} style={{ flex: 1, padding: '6px 12px', background: '#20b380', border: 'none', borderRadius: 6, color: '#fff', cursor: 'pointer', fontSize: 13 }}>+ 新建</button>}
        </div>
        {!sidebarCollapsed && Object.values(sessions).map(s => (
          <div key={s.id} onClick={() => setActiveSessionId(s.id)}
            style={{ padding: '6px 8px', cursor: 'pointer', background: s.id === activeSessionId ? '#21262d' : 'transparent', borderRadius: 6, marginTop: 4, fontSize: 13 }}>
            {s.title || s.id}
            <button onClick={(e) => { e.stopPropagation(); handleDeleteSession(s.id); }}
              style={{ float: 'right', background: 'none', border: 'none', color: '#f85149', cursor: 'pointer', fontSize: 11 }}>x</button>
          </div>
        ))}
        <div style={{ position: 'absolute', bottom: 12, fontSize: 11, color: connected ? '#3fb950' : '#f85149' }}>
          {connected ? '已连接' : '未连接'}
        </div>
      </div>

      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
          {globalError && (
            <div style={{ padding: 12, background: '#490202', borderRadius: 8, marginBottom: 12, color: '#f85149', fontSize: 13 }}>{globalError}</div>
          )}
          {activeSession ? (
            <div>
              <div style={{ marginBottom: 16 }}>
                <h2 style={{ fontSize: 18, margin: 0 }}>{activeSession.title || activeSession.id}</h2>
                <span style={{ fontSize: 12, color: '#8b949e' }}>{activeSession.status}</span>
              </div>
              {activeSession.messages.map((msg, i) => (
                <div key={i} style={{ marginBottom: 8, padding: 12, background: '#161b22', borderRadius: 8, fontSize: 13, maxWidth: '100%', overflow: 'auto' }}>
                  <div style={{ color: '#58a6ff', marginBottom: 4, fontWeight: 600 }}>{typeof msg.type === 'string' ? msg.type : 'message'}</div>
                  <pre style={{ margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word', maxHeight: 300, overflow: 'auto' }}>
                    {JSON.stringify(msg, null, 2).slice(0, 2000)}
                  </pre>
                </div>
              ))}
            </div>
          ) : (
            <div style={{ textAlign: 'center', marginTop: 100 }}>
              <h1 style={{ fontSize: 28, marginBottom: 8 }}>Aegis Desktop</h1>
              <p style={{ color: '#8b949e', marginBottom: 4 }}>{connected ? '已连接后端' : '未连接'} | {Object.keys(sessions).length} 个会话</p>
              <p style={{ color: '#8b949e', fontSize: 13 }}>
                API Key: {providerConfigs.deepseek.apiKey ? '已配置' : '未配置'}
              </p>
              {!providerConfigs.deepseek.apiKey && (
                <p style={{ color: '#f85149', fontSize: 12, marginTop: 8 }}>请在侧边栏设置中配置 API Key，或创建 ~/.aegis/config.toml</p>
              )}
            </div>
          )}
        </div>

        {activeSession && (
          <div style={{ borderTop: '1px solid #21262d', padding: 12 }}>
            <div style={{ display: 'flex', gap: 8 }}>
              <input value={prompt} onChange={e => setPrompt(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleContinue(); } }}
                placeholder="输入消息，Enter 发送，Shift+Enter 换行..."
                style={{ flex: 1, padding: '10px 14px', background: '#161b22', border: '1px solid #30363d', borderRadius: 8, color: '#c9d1d9', fontSize: 14, outline: 'none' }} />
              <button onClick={handleContinue}
                style={{ padding: '10px 20px', background: '#20b380', border: 'none', borderRadius: 8, color: '#fff', cursor: 'pointer', fontWeight: 600, whiteSpace: 'nowrap' }}>
                发送
              </button>
            </div>
          </div>
        )}
      </div>

      {showNewModal && (
        <div style={{ position: 'fixed', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', background: 'rgba(0,0,0,0.5)', zIndex: 50 }}
          onClick={() => setShowNewModal(false)}>
          <div style={{ background: '#161b22', borderRadius: 12, padding: 24, width: 480, maxHeight: '80vh', overflow: 'auto' }}
            onClick={e => e.stopPropagation()}>
            <h2 style={{ fontSize: 18, marginBottom: 16, marginTop: 0 }}>新建会话</h2>
            <div style={{ marginBottom: 12, fontSize: 12, color: '#8b949e' }}>
              API Key: {providerConfigs.deepseek.apiKey ? '已配置 (' + providerConfigs.deepseek.model + ')' : '未配置'}
            </div>
            <label style={{ display: 'block', marginBottom: 12 }}>
              <div style={{ fontSize: 12, color: '#8b949e', marginBottom: 4 }}>工作目录</div>
              <input value={cwd} onChange={e => setCwd(e.target.value)} placeholder="留空使用当前目录"
                style={{ width: '100%', padding: '8px 12px', background: '#0d1117', border: '1px solid #30363d', borderRadius: 8, color: '#c9d1d9', fontSize: 14, outline: 'none', boxSizing: 'border-box' }} />
            </label>
            <label style={{ display: 'block', marginBottom: 16 }}>
              <div style={{ fontSize: 12, color: '#8b949e', marginBottom: 4 }}>提示词</div>
              <textarea rows={4} value={prompt} onChange={e => setPrompt(e.target.value)} placeholder="描述你想让 Agent 处理的任务..."
                style={{ width: '100%', padding: '8px 12px', background: '#0d1117', border: '1px solid #30363d', borderRadius: 8, color: '#c9d1d9', fontSize: 14, outline: 'none', resize: 'vertical', boxSizing: 'border-box' }} />
            </label>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button onClick={() => setShowNewModal(false)}
                style={{ padding: '8px 16px', background: '#21262d', border: 'none', borderRadius: 8, color: '#c9d1d9', cursor: 'pointer' }}>取消</button>
              <button onClick={handleNewSession} disabled={!prompt.trim()}
                style={{ padding: '8px 16px', background: '#20b380', border: 'none', borderRadius: 8, color: '#fff', cursor: prompt.trim() ? 'pointer' : 'not-allowed', opacity: prompt.trim() ? 1 : 0.5 }}>开始会话</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
