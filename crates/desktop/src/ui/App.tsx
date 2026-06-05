import { useEffect, useState, useCallback, useRef } from "react";
import { useIPC } from "./hooks/useIPC";
import { useAppStore } from "./store/useAppStore";
import type { ServerEvent, ClientEvent } from "./types";
import { Sidebar } from "./components/Sidebar";
import MDContent from "./render/markdown";

function App() {
  const messagesEndRef = useRef<HTMLDivElement>(null);
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
  const [pendingSend, setPendingSend] = useState(false);

  const activeSession = activeSessionId ? sessions[activeSessionId] : null;
  const isRunning = activeSession?.status === "running";

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeSession?.messages]);

  const handleNewSession = useCallback(() => {
    const cfg = providerConfigs.deepseek;
    if (!cfg.apiKey) { alert("请先在设置中配置 API Key"); return; }
    const name = projectName.trim() || (cwd ? cwd.split(/[\\/]/).pop() || cwd : "新项目");
    setPendingSend(true);
    sendEvent({
      type: "session.start",
      payload: { title: name, prompt: prompt || "Hello", cwd: cwd || undefined, provider: "deepseek", apiKey: cfg.apiKey, model: cfg.model },
    });
    setShowNewModal(false);
    setProjectName("");
    setPrompt("");
  }, [sendEvent, projectName, prompt, cwd, providerConfigs]);

  const handleContinue = useCallback(() => {
    if (!prompt.trim() || !activeSessionId) return;
    sendEvent({ type: "session.continue", payload: { sessionId: activeSessionId, prompt: prompt.trim() } });
    setPrompt("");
  }, [sendEvent, prompt, activeSessionId]);

  const handleDeleteSession = useCallback((id: string) => {
    sendEvent({ type: "session.delete", payload: { sessionId: id } });
  }, [sendEvent]);

  // Clear pending when session status changes
  useEffect(() => {
    if (activeSession && activeSession.status !== "running" && pendingSend) {
      setPendingSend(false);
    }
  }, [activeSession?.status]);

  return (
    <div className="flex h-screen bg-surface text-ink-900">
      <Sidebar
        connected={connected}
        collapsed={sidebarCollapsed}
        onNewSession={() => setShowNewModal(true)}
        onDeleteSession={handleDeleteSession}
        onToggleCollapse={() => setSidebarCollapsed(!sidebarCollapsed)}
      />

      <div className="flex flex-1 flex-col min-w-0">
        {/* Messages area */}
        <div className="flex-1 overflow-auto px-6 py-4">
          {globalError && (
            <div className="mb-4 rounded-xl bg-red-900/30 border border-red-800 px-4 py-3 text-sm text-red-300">
              {globalError}
              <button className="ml-3 underline" onClick={() => setGlobalError(null)}>Dismiss</button>
            </div>
          )}

          {activeSession ? (
            <div>
              <div className="mb-6">
                <h2 className="text-lg font-semibold text-ink-800">{activeSession.title || activeSession.id}</h2>
                <span className="text-xs text-muted">{activeSession.cwd || '默认目录'} · {activeSession.status}</span>
              </div>

              {activeSession.messages.map((msg: Record<string, unknown>, i: number) => {
                const msgType = msg.type as string;
                if (msgType === "user_prompt") return (
                  <div key={i} className="mb-5 ml-8">
                    <div className="text-xs text-accent mb-1 font-medium">You</div>
                    <div className="rounded-2xl rounded-tr-md bg-surface-secondary border border-ink-900/10 px-5 py-3 text-sm leading-relaxed">
                      <MDContent text={String(msg.prompt ?? "")} />
                    </div>
                  </div>
                );
                if (msgType === "assistant") return (
                  <div key={i} className="mb-5 mr-8">
                    <div className="rounded-2xl rounded-tl-md bg-panel border border-ink-900/10 px-5 py-3 text-sm leading-relaxed">
                      <MDContent text={String(msg.text ?? "")} />
                    </div>
                  </div>
                );
                if (msgType === "thinking") return (
                  <div key={i} className="mb-3 mx-8">
                    <details className="text-xs text-muted">
                      <summary className="cursor-pointer italic py-1">Thinking...</summary>
                      <div className="mt-2 pl-3 border-l-2 border-ink-900/20 whitespace-pre-wrap">{msg.text as string}</div>
                    </details>
                  </div>
                );
                if (msgType === "tool_use") return (
                  <div key={i} className="mb-3 mx-8">
                    <div className={`rounded-lg border px-3 py-2 text-xs flex items-center gap-2 ${
                      msg.status === "error" ? "border-red-800 bg-red-900/20 text-red-300" :
                      msg.status === "success" ? "border-green-800 bg-green-900/20 text-green-300" :
                      "border-ink-900/20 bg-surface-tertiary text-ink-700"
                    }`}>
                      <span className="font-semibold">{msg.name as string}</span>
                      <span className="text-muted">{msg.status as string}</span>
                      {msg.elapsed_ms ? <span className="text-muted ml-auto">{(msg.elapsed_ms as number)}ms</span> : null}
                    </div>
                    {msg.output ? (
                      <details className="mt-1">
                        <summary className="text-xs text-muted cursor-pointer">Output</summary>
                        <pre className="mt-1 p-3 rounded-lg bg-surface-tertiary text-xs whitespace-pre-wrap max-h-48 overflow-auto">{msg.output as string}</pre>
                      </details>
                    ) : null}
                  </div>
                );
                if (msgType === "usage") return (
                  <div key={i} className="mb-6 text-center">
                    <span className="text-xs text-muted bg-surface-secondary rounded-full px-4 py-1">
                      Tokens: {(msg as Record<string,unknown>).input_tokens as number} in / {(msg as Record<string,unknown>).output_tokens as number} out
                      · Cost: ${Number((msg as Record<string,unknown>).cost).toFixed(4)}
                    </span>
                  </div>
                );
                return null;
              })}

              {isRunning && (
                <div className="flex items-center gap-3 ml-8 mb-5 text-sm text-muted">
                  <span className="flex h-2 w-2"><span className="animate-ping absolute inline-flex h-2 w-2 rounded-full bg-accent opacity-75"/><span className="relative inline-flex rounded-full h-2 w-2 bg-accent"/></span>
                  Agent is working...
                </div>
              )}
              <div ref={messagesEndRef} />
            </div>
          ) : (
            <div className="flex flex-col items-center justify-center h-full text-center -mt-20">
              <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-accent/10 text-2xl font-bold text-accent mb-4">ag</div>
              <h1 className="text-2xl font-semibold text-ink-800 mb-2">Aegis Desktop</h1>
              <p className="text-muted text-sm mb-1">{connected ? '已连接后端' : '未连接'}</p>
              <p className="text-muted text-xs mb-6">
                API Key: {providerConfigs.deepseek.apiKey ? '已配置 (' + providerConfigs.deepseek.model + ')' : '未配置 — 请点侧边栏齿轮图标设置'}
              </p>
              <button onClick={() => setShowNewModal(true)}
                className="rounded-full bg-accent px-6 py-3 text-sm font-medium text-white hover:bg-accent-hover transition-colors">
                + 新建项目
              </button>
            </div>
          )}
        </div>

        {/* Input bar */}
        {activeSession && (
          <div className="border-t border-ink-900/10 px-6 py-3">
            <div className="flex gap-3 items-end">
              <textarea
                value={prompt}
                onChange={e => setPrompt(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleContinue(); }
                }}
                placeholder={isRunning ? "Agent is working..." : "输入消息，Enter 发送，Shift+Enter 换行..."}
                rows={1}
                disabled={isRunning}
                className="flex-1 resize-none rounded-xl border border-ink-900/10 bg-surface-secondary px-4 py-3 text-sm text-ink-800 placeholder:text-muted-light focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent/20 disabled:opacity-50"
                style={{ minHeight: 44, maxHeight: 200 }}
                onInput={e => {
                  const el = e.currentTarget;
                  el.style.height = 'auto';
                  el.style.height = Math.min(el.scrollHeight, 200) + 'px';
                }}
              />
              <button
                onClick={handleContinue}
                disabled={!prompt.trim() || isRunning}
                className="rounded-xl bg-accent px-5 py-3 text-sm font-medium text-white hover:bg-accent-hover transition-colors disabled:opacity-40 disabled:cursor-not-allowed shrink-0">
                发送
              </button>
            </div>
          </div>
        )}
      </div>

      {/* New project modal */}
      {showNewModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
          onClick={() => setShowNewModal(false)}>
          <div className="bg-panel rounded-2xl border border-ink-900/20 p-6 w-full max-w-md shadow-2xl"
            onClick={e => e.stopPropagation()}>
            <h2 className="text-lg font-semibold text-ink-800 mb-4">新建项目</h2>
            <div className="mb-3 text-xs text-muted">
              API Key: {providerConfigs.deepseek.apiKey ? '已配置 (' + providerConfigs.deepseek.model + ')' : '未配置'}
            </div>
            <input
              value={projectName}
              onChange={e => setProjectName(e.target.value)}
              placeholder="项目名称（留空使用目录名）"
              className="w-full mb-3 rounded-xl border border-ink-900/10 bg-surface-secondary px-4 py-2.5 text-sm text-ink-800 placeholder:text-muted-light focus:border-accent focus:outline-none"
            />
            <input
              value={cwd}
              onChange={e => setCwd(e.target.value)}
              placeholder="工作目录（留空使用当前目录）"
              className="w-full mb-3 rounded-xl border border-ink-900/10 bg-surface-secondary px-4 py-2.5 text-sm text-ink-800 placeholder:text-muted-light focus:border-accent focus:outline-none"
            />
            <textarea
              rows={3}
              value={prompt}
              onChange={e => setPrompt(e.target.value)}
              placeholder="初始提示词（可选）"
              className="w-full mb-4 rounded-xl border border-ink-900/10 bg-surface-secondary px-4 py-3 text-sm text-ink-800 placeholder:text-muted-light focus:border-accent focus:outline-none resize-none"
            />
            <div className="flex gap-3 justify-end">
              <button onClick={() => setShowNewModal(false)}
                className="rounded-full px-5 py-2 text-sm text-muted hover:text-ink-700 transition-colors">取消</button>
              <button onClick={handleNewSession}
                className="rounded-full bg-accent px-5 py-2 text-sm font-medium text-white hover:bg-accent-hover transition-colors">创建项目</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
