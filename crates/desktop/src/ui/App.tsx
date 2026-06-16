import { useEffect, useState, useCallback, useRef, useMemo } from "react";
import { useIPC } from "./hooks/useIPC";
import { useAppStore } from "./store/useAppStore";
import { I } from "./icons";
import { Thread } from "./render/Thread";
import { SettingsModal } from "./components/SettingsModal";
import { AegisLogo } from "./render/AegisLogo";
import { Sidebar } from "./render/Sidebar";
import { Composer } from "./components/Composer";
import { CommandPalette } from "./components/CommandPalette";
import { NewSessionModal, AboutModal } from "./components/NewSessionModal";
import { StatusBar } from "./components/StatusBar";
import { ToastContainer, nextToastId, type ToastItem } from "./components/Toast";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ConnectPhoneModal } from "./components/ConnectPhoneModal";
import { SessionHeader } from "./components/SessionHeader";
import { ChatStarterGrid } from "./components/ChatStarterGrid";
import { RuntimeBanner } from "./components/RuntimeBanner";
import { PreviewPanel } from "./components/PreviewPanel";
import { FileTree } from "./render/FileTree";
import { GraphPanel } from "./components/GraphPanel";
import { SessionInfoPanel } from "./components/SessionInfoPanel";

/* ── Types ─────────────────────────────────────────────────── */

type Theme = "light" | "dark";
type SlashCmd = { cmd: string; desc: string; icon: React.ReactNode; run: () => void };
type RealSkill = { name: string; description: string };

// Default permission mode — synced with backend, user can change in settings
const DEFAULT_MODE = "default";

interface SettingsFields { apiKey: string; model: string; }

/* ── Theme hook ────────────────────────────────────────────── */

function useTheme() {
  const [theme, setTheme] = useState<Theme>(() => {
    if (typeof window === "undefined") return "light";
    return (localStorage.getItem("aegis-theme") as Theme) ?? "light";
  });
  useEffect(() => { document.documentElement.setAttribute("data-theme", theme); localStorage.setItem("aegis-theme", theme); }, [theme]);
  return { theme, toggle: () => setTheme(t => t === "light" ? "dark" : "light") };
}

/* ── Time formatter ────────────────────────────────────────── */

function relativeTime(ts: number): string {
  const ms = ts < 1e12 ? ts * 1000 : ts;
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "刚刚";
  if (s < 3600) return `${Math.floor(s / 60)}分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)}小时前`;
  return `${Math.floor(s / 86400)}天前`;
}

/* ── App Shell ─────────────────────────────────────────────── */

function App() {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesAreaRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const { theme, toggle: toggleTheme } = useTheme();

  const initConfig = useAppStore(s => s.initConfig);
  useEffect(() => { initConfig(); }, []);

  const handleServerEvent = useAppStore(s => s.handleServerEvent);
  const { connected, sendEvent } = useIPC(handleServerEvent);

  // ── IM bridge: listen for Feishu messages ────────────────────
  const pendingImReplyRef = useRef<string | null>(null);
  const sendEventRef = useRef(sendEvent);
  sendEventRef.current = sendEvent;

  useEffect(() => {
    const tauri = window.__TAURI__;
    if (!tauri?.event?.listen) return;

    let cancelled = false;
    let unlistenFn: (() => void) | null = null;

    tauri.event.listen<{chatId: string; text: string; sender: string; platform: string}>("im-message", (event) => {
      if (cancelled) return;
      const { chatId, text } = event.payload;
      const state = useAppStore.getState();
      const sessionId = state.activeSessionId;
      if (!sessionId) return;

      const session = state.sessions[sessionId];
      if (session?.status === "running") return;

      pendingImReplyRef.current = chatId;
      const cwd = state.sessions[sessionId]?.cwd;
      sendEventRef.current({ type: "session.continue", payload: { sessionId, prompt: text, cwd } });
    }).then(fn => {
      if (cancelled) { fn(); return; }
      unlistenFn = fn;
    });

    return () => {
      cancelled = true;
      unlistenFn?.();
    };
  }, []);

  const sessions = useAppStore(s => s.sessions);
  const activeSessionId = useAppStore(s => s.activeSessionId);
  const setActiveSessionId = useAppStore(s => s.setActiveSessionId);
  const loadProjectSessions = useAppStore(s => s.loadProjectSessions);

  // When agent finishes an IM-triggered turn, send reply back to Feishu
  const imReplySentRef = useRef(false);
  const prevStatusRef = useRef<string>("");
  useEffect(() => {
    const chatId = pendingImReplyRef.current;
    if (!chatId || !activeSessionId) return;
    const session = sessions[activeSessionId];
    if (!session) return;

    const wasRunning = prevStatusRef.current === "running";
    prevStatusRef.current = session.status;

    if (wasRunning && session.status === "completed" && !imReplySentRef.current) {
      imReplySentRef.current = true;
      const msgs = session.messages || [];
      for (let i = msgs.length - 1; i >= 0; i--) {
        const m = msgs[i] as Record<string, unknown>;
        if (m.type === "assistant" && m.text && typeof m.text === "string" && (m.text as string).length > 0) {
          window.__TAURI__?.core?.invoke("send_im_reply", {
            chatId, text: m.text as string,
          }).catch(() => {});
          break;
        }
      }
      pendingImReplyRef.current = null;
    }

    // Reset when not running (new turn can start)
    if (session.status !== "running") {
      imReplySentRef.current = false;
    }
  }, [sessions, activeSessionId]);

  const handleSelectSession = useCallback(async (id: string) => {
    setActiveSessionId(id);
    const session = sessions[id];
    window.__TAURI__?.core?.invoke("notify_im_project", {
      cwd: session?.cwd || id,
      sessionId: id,
    }).catch((e: any) => console.error("[IM] notify_im_project failed:", e));
    if (session && !session.hydrated && session.cwd) {
      await loadProjectSessions(session.cwd);
    }
  }, [setActiveSessionId, sessions, loadProjectSessions]);

  const providerConfigs = useAppStore(s => s.providerConfigs);
  const setProviderConfig = useAppStore(s => s.setProviderConfig);
  const globalError = useAppStore(s => s.globalError);
  const setGlobalError = useAppStore(s => s.setGlobalError);

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [sidebarWidth, setSidebarWidth] = useState(240);
  const [contextPanelWidth, setContextPanelWidth] = useState(300);
  const resizing = useRef<"sidebar" | "panel" | null>(null);
  const [showNewModal, setShowNewModal] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showPhoneConnect, setShowPhoneConnect] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [showCmdPalette, setShowCmdPalette] = useState(false);
  const [cwd, setCwd] = useState("");
  const [projectName, setProjectName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [sidebarSearch, setSidebarSearch] = useState("");
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [confirmDelete, setConfirmDelete] = useState<{ id: string; cwd?: string; kind: "session" | "project" } | null>(null);
  const [selectedModel, setSelectedModel] = useState(providerConfigs.deepseek.model);
  const [reasoningEffort, setReasoningEffort] = useState("max");
  const [loadedSkills, setLoadedSkills] = useState<RealSkill[]>([]);
  const [scanning, setScanning] = useState(false);

  const initProject = useAppStore(s => s.initProject);
  const scanProject = useAppStore(s => s.scanProject);
  const checkProject = useAppStore(s => s.checkProject);
  const scanResult = useAppStore(s => s.scanResult);

  const addToast = useCallback((text: string, kind: ToastItem["kind"] = "info") => {
    const id = nextToastId();
    setToasts(t => [...t, { id, text, kind }]);
    setTimeout(() => setToasts(t => t.filter(x => x.id !== id)), 3000);
  }, []);
  const removeToast = useCallback((id: number) => { setToasts(t => t.filter(x => x.id !== id)); }, []);

  const activeSession = activeSessionId ? sessions[activeSessionId] : null;
  const isRunning = activeSession?.status === "running";
  const cfg = providerConfigs.deepseek;

  // Cumulative usage stats from all messages in active session
  const cumulativeUsage = useMemo(() => {
    let input_tokens = 0, output_tokens = 0, cache_tokens = 0, cost = 0;
    for (const m of activeSession?.messages ?? []) {
      if (m.type === "usage") {
        input_tokens += (m as Record<string,number>).input_tokens || 0;
        output_tokens += (m as Record<string,number>).output_tokens || 0;
        cache_tokens += (m as Record<string,number>).cache_read_tokens || 0;
        cost += (m as Record<string,number>).cost || 0;
      }
    }
    return { input_tokens, output_tokens, cache_tokens, cost };
  }, [activeSession?.messages]);

  // Scroll to bottom on session switch
  useEffect(() => {
    isAtBottomRef.current = true;
    // Delay to let DOM render before scrolling
    const id = setTimeout(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: "instant" as ScrollBehavior });
    }, 50);
    return () => clearTimeout(id);
  }, [activeSessionId]);

  // Smart scroll: only auto-scroll when user is at bottom
  useEffect(() => {
    if (!messagesAreaRef.current || !isAtBottomRef.current) return;
    messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
  }, [activeSession?.messages]);

  const handleMessagesScroll = useCallback(() => {
    const el = messagesAreaRef.current;
    if (!el) return;
    const threshold = 40;
    isAtBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
  }, []);

  // Load skills for active session's workspace
  useEffect(() => {
    if (!activeSession?.cwd) return;
    const load = async () => {
      try {
        const skills = await window.__TAURI__?.core?.invoke<RealSkill[]>("list_skills", { cwd: activeSession.cwd });
        if (skills) setLoadedSkills(skills);
      } catch { /* no skills available */ }
    };
    load();
  }, [activeSession?.cwd]);

  // Keyboard shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") { e.preventDefault(); setShowCmdPalette(true); }
      if ((e.metaKey || e.ctrlKey) && e.key === "b") { e.preventDefault(); setSidebarCollapsed(c => !c); }
      if ((e.metaKey || e.ctrlKey) && e.key === ",") { e.preventDefault(); setShowSettings(true); }
      if (e.key === "Escape") { setShowCmdPalette(false); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Refs for direct DOM resize (bypass React re-render during drag)
  const sidebarRef = useRef<HTMLDivElement>(null);
  const panelWrapperRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // Resize handlers — direct DOM write during drag, commit to state on release
  useEffect(() => {
    let raf = 0;
    const onMouseMove = (e: MouseEvent) => {
      if (!resizing.current) return;
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        if (resizing.current === "sidebar" && sidebarRef.current) {
          const w = Math.max(180, Math.min(420, e.clientX));
          sidebarRef.current.style.width = `${w}px`;
          sidebarRef.current.style.minWidth = `${w}px`;
        } else if (resizing.current === "panel" && panelRef.current && panelWrapperRef.current) {
          const w = Math.max(280, Math.min(600, window.innerWidth - e.clientX));
          panelRef.current.style.width = `${w}px`;
          panelRef.current.style.minWidth = `${w}px`;
          panelWrapperRef.current.style.minWidth = `${w + 6}px`;
        }
      });
    };
    const onMouseUp = () => {
      document.body.classList.remove("resizing");
      if (resizing.current === "sidebar" && sidebarRef.current) {
        const w = parseInt(sidebarRef.current.style.width || "240", 10);
        setSidebarWidth(w);
      } else if (resizing.current === "panel" && panelRef.current && panelWrapperRef.current) {
        const w = parseInt(panelRef.current.style.width || "300", 10);
        setContextPanelWidth(w);
        panelWrapperRef.current.style.minWidth = "306px";
      }
      resizing.current = null;
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
      cancelAnimationFrame(raf);
    };
  }, []);

  /* ── Handlers ──────────────────────────────────────────── */

  const handleNewSession = useCallback(async () => {
    if (!cfg.apiKey) { addToast("请先在设置中配置 API Key", "error"); return; }
    const name = projectName.trim() || (cwd ? cwd.split(/[\\/]/).pop() || cwd : "新项目");
    const workDir = cwd || ".";

    setScanning(true);

    const exists = await checkProject(workDir);
    if (exists) {
      await loadProjectSessions(workDir);
      setActiveSessionId(workDir);
      setScanning(false);
      setShowNewModal(false); setProjectName(""); setPrompt("");
      addToast("已打开现有项目", "success");
      return;
    }

    // New project: init + scan
    const meta = await initProject(workDir);
    if (meta) {
      addToast(`项目已创建 · ${meta.language || "unknown"} · ${meta.file_count} 文件`, "success");
    }
    const result = await scanProject(workDir);
    if (result) {
      addToast(`扫描完成 · ${result.total_files} 文件 · ${result.duration_ms}ms`, "info");
    }
    setScanning(false);

    sendEvent({ type: "session.start", payload: { title: name, prompt: prompt.trim(), cwd: workDir || undefined, provider: "deepseek", apiKey: cfg.apiKey, model: selectedModel, executionMode: "yolo" } });
    setShowNewModal(false); setProjectName(""); setPrompt("");
  }, [sendEvent, projectName, prompt, cwd, cfg, selectedModel, addToast, initProject, scanProject, checkProject, loadProjectSessions, setActiveSessionId]);

  const handleContinue = useCallback((text: string) => {
    if (!activeSessionId || isRunning) return;
    // Clear any stale IM reply reference — this is a manual desktop message
    pendingImReplyRef.current = null;
    const session = sessions[activeSessionId];
    // /goal handling: set contract first, then send the goal as prompt
    if ((window as any).__aegisGoal) {
      const { objective, criteria } = (window as any).__aegisGoal;
      delete (window as any).__aegisGoal;
      sendEvent({ type: "session.goal", payload: { sessionId: activeSessionId, objective, criteria } });
    }
    sendEvent({ type: "session.continue", payload: { sessionId: activeSessionId, prompt: text, messages: session?.messages ?? [], cwd: session?.cwd } });
  }, [sendEvent, activeSessionId, isRunning, sessions]);

  const handleStop = useCallback(() => { if (activeSessionId) { sendEvent({ type: "session.stop", payload: { sessionId: activeSessionId } }); addToast("已停止"); } }, [sendEvent, activeSessionId, addToast]);

  const handleDeleteSession = useCallback((id: string) => {
    const session = sessions[id];
    setConfirmDelete({ id, cwd: session?.cwd, kind: "session" });
  }, [sessions]);

  const handleDeleteProject = useCallback((cwd: string) => {
    setConfirmDelete({ id: cwd, cwd, kind: "project" });
  }, []);

  const confirmDeleteChoice = useCallback(async (idx: number) => {
    if (!confirmDelete) return;
    const deleteProject = confirmDelete.kind === "project" || idx === 1;
    if (deleteProject) {
      // Delete project data
      const cwd = confirmDelete.cwd;
      if (cwd) {
        try { await window.__TAURI__?.core?.invoke("delete_project", { cwd }); } catch {}
      }
      const next = { ...sessions };
      for (const [id, s] of Object.entries(next)) {
        if (s.cwd === cwd) delete next[id];
      }
      useAppStore.setState({ sessions: next });
      if (activeSessionId && !next[activeSessionId]) setActiveSessionId(null);
      addToast("已删除项目数据");
    } else {
      // Delete session only
      const cwd = confirmDelete.cwd;
      if (cwd) {
        try { await window.__TAURI__?.core?.invoke("delete_session", { cwd, sessionId: confirmDelete.id }); } catch {}
      }
      const next = { ...sessions }; delete next[confirmDelete.id];
      useAppStore.setState({ sessions: next });
      if (activeSessionId === confirmDelete.id) setActiveSessionId(null);
      addToast("已删除对话");
    }
    setConfirmDelete(null);
  }, [confirmDelete, sessions, activeSessionId, addToast, setActiveSessionId]);

  const handleRenameSession = useCallback((id: string, title: string) => {
    const session = sessions[id]; if (session) { useAppStore.setState({ sessions: { ...sessions, [id]: { ...session, title } } }); addToast("已重命名"); }
  }, [sessions, addToast]);

  const handleSaveSettings = useCallback((f: SettingsFields) => {
    setProviderConfig("deepseek", { apiKey: f.apiKey, model: f.model, baseUrl: cfg.baseUrl });
    setSelectedModel(f.model); addToast("设置已保存", "success");
  }, [setProviderConfig, cfg.baseUrl, addToast]);

  const handleModelChange = useCallback((m: string) => { setSelectedModel(m); setProviderConfig("deepseek", { ...cfg, model: m }); addToast(`模型: ${m}`); }, [cfg, setProviderConfig, addToast]);

  /* ── Slash commands ───────────────────────────────────── */

  const slashCommands = useMemo((): SlashCmd[] => [
    { cmd: "/compact",    desc: "压缩会话上下文以释放 token",    icon: <I.copy />,     run: () => {
      if (!activeSessionId) { addToast("无活动会话", "error"); return; }
      sendEvent({ type: "session.compact", payload: { sessionId: activeSessionId } });
      addToast("正在压缩…");
    }},
    { cmd: "/clear",      desc: "清空当前对话上下文",           icon: <I.x />,       run: () => {
      if (!activeSessionId) return;
      sendEvent({ type: "session.clear", payload: { sessionId: activeSessionId } });
      setPrompt("");
    }},
    { cmd: "/resume",     desc: "恢复之前的会话",               icon: <I.folder />,   run: () => setShowCmdPalette(true) },
    { cmd: "/goal",       desc: "设定验收目标（/goal 目标 | 标准1, 标准2）", icon: <I.zap />, run: () => setPrompt("/goal ") },
    { cmd: "/export",     desc: "导出当前会话到剪贴板",       icon: <I.download />, run: async () => {
      if (!activeSession) { addToast("无活动会话", "error"); return; }
      const ts = new Date().toISOString().slice(0, 19).replace(/:/g, "-");
      const msgs = activeSession.messages.map(m => {
        if (m.type === "user_prompt") return `**▸ You**\n\n${(m as Record<string,unknown>).prompt || ""}`;
        if (m.type === "assistant") return `*** Aegis**\n\n${(m as Record<string,unknown>).text || ""}`;
        return "";
      }).join("\n\n");
      try { await navigator.clipboard.writeText(msgs); addToast(`已复制 ${activeSession.messages.length} 条消息到剪贴板`, "success"); } catch { addToast("复制失败", "error"); }
    }},
  ], [activeSessionId, activeSession, sendEvent, addToast]);

  const paletteCommands = useMemo(() => {
    const map = slashCommands.reduce((acc, c) => { acc[c.cmd.slice(1)] = c; return acc; }, {} as Record<string, SlashCmd>);
    return Object.values(map).map(c => ({ id: c.cmd.slice(1), label: c.cmd, desc: c.desc, icon: c.icon, run: c.run }));
  }, [slashCommands]);

  /* ── Context Panel state ────────────────────────────── */

  const [showContextPanel, setShowContextPanel] = useState(false);
  const [ctxTab, setCtxTab] = useState<"info" | "graph" | "files" | "preview">("info");
  const [previewUrl, setPreviewUrl] = useState("");

  // Auto-detect dev server URLs in agent output → open preview panel
  useEffect(() => {
    if (!activeSession) return;
    const msgs = activeSession.messages;
    if (msgs.length === 0) return;
    // Check last 5 messages for localhost URLs
    for (let i = msgs.length - 1; i >= Math.max(0, msgs.length - 5); i--) {
      const m = msgs[i];
      const text = (m as Record<string,unknown>).text as string
                || (m as Record<string,unknown>).output as string
                || (m as Record<string,unknown>).prompt as string
                || "";
      const match = text.match(/https?:\/\/localhost:\d+/);
      if (match) {
        if (previewUrl !== match[0]) {
          setPreviewUrl(match[0]);
          setShowContextPanel(true);
          setCtxTab("preview");
        }
        return;
      }
    }
  }, [activeSession?.messages]);

  return (
    <div className="app-shell">
      <div ref={sidebarRef} style={sidebarCollapsed ? { width: 52, flexShrink: 0 } : { width: sidebarWidth, minWidth: 180, flexShrink: 0 }}>
        <Sidebar collapsed={sidebarCollapsed} onToggle={() => setSidebarCollapsed(!sidebarCollapsed)}
          sessions={sessions} activeSessionId={activeSessionId}
          onSelect={handleSelectSession} onDelete={handleDeleteSession} onRename={handleRenameSession}
          onDeleteProject={handleDeleteProject}
          onNew={() => setShowNewModal(true)}
          onOpenSettings={() => setShowSettings(true)}
          onOpenPhoneConnect={() => setShowPhoneConnect(true)}
          onOpenAbout={() => setShowAbout(true)}
          connected={connected} model={cfg.apiKey ? selectedModel : "未配置 Key"} search={sidebarSearch} setSearch={setSidebarSearch} />
      </div>
      {!sidebarCollapsed && (
        <div className="resize-handle" onMouseDown={() => { resizing.current = "sidebar"; document.body.classList.add("resizing"); }} />
      )}

      <div className="layout-main">
        <div className="main-area">

          {/* Session Header — glass topbar */}
          {activeSession ? (
            <SessionHeader
              title={activeSession.title || activeSession.id}
              cwd={activeSession.cwd}
              model={selectedModel}
              isRunning={isRunning}
              status={activeSession.status}
              updatedAt={activeSession.updatedAt}
              onRename={(title) => handleRenameSession(activeSessionId!, title)}
              onToggleContextPanel={() => setShowContextPanel(p => !p)}
              onToggleTheme={toggleTheme}
              onOpenCmdPalette={() => setShowCmdPalette(true)}
              isDark={theme === "dark"}
            />
          ) : (
            <div className="top-bar">
              <span className="top-bar-left" />
              <div className="top-bar-actions">
                <button className="pill-btn" onClick={() => setShowCmdPalette(true)} title="命令面板"><I.command /> Ctrl+K</button>
                <button className="pill-btn" onClick={toggleTheme} title={theme === "light" ? "深色" : "亮色"}>{theme === "light" ? <I.moon /> : <I.sun />}</button>
              </div>
            </div>
          )}

          {/* Runtime error banner */}
          <RuntimeBanner error={globalError} onDismiss={() => setGlobalError(null)} />

          <div className="messages-area" ref={messagesAreaRef} onScroll={handleMessagesScroll}>
            {activeSession ? (
              <Thread messages={activeSession.messages} isRunning={isRunning} />
            ) : (
              <div className="welcome-section">
                <div className="welcome-logo-wrapper"><AegisLogo size={64} /></div>
                <div className="welcome-heading">欢迎使用 Aegis</div>
                <div className="welcome-sub">
                  {!cfg.apiKey
                    ? "请先配置 DeepSeek API Key 以开始使用"
                    : "选择以下操作开始，或从左侧打开历史项目"}
                </div>
                <ChatStarterGrid
                  onNew={() => setShowNewModal(true)}
                  onSettings={() => setShowSettings(true)}
                />
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>

          {activeSession && (
            <Composer prompt={prompt} setPrompt={setPrompt} onSubmit={handleContinue} onStop={handleStop}
              isRunning={isRunning} model={selectedModel}
              onModelChange={handleModelChange}
              reasoningEffort={reasoningEffort} onReasoningChange={setReasoningEffort}
              cwd={activeSession.cwd} inputRef={inputRef}
              slashCommands={slashCommands} skills={loadedSkills} />
          )}
        </div>

        <StatusBar
          inputTokens={cumulativeUsage.input_tokens}
          outputTokens={cumulativeUsage.output_tokens}
          cost={cumulativeUsage.cost}
          cachePct={activeSession?.cachePct}
          isRunning={isRunning}
          model={selectedModel}
          connected={connected}
        />
      </div>

      {/* Context panel (right sidebar) */}
      {activeSession && (
        <div ref={panelWrapperRef} className={`context-panel-wrapper ${showContextPanel ? "open" : ""}`}>
          <div className="resize-handle" onMouseDown={() => { resizing.current = "panel"; document.body.classList.add("resizing"); }} />
          <div ref={panelRef} className="context-panel" style={{ width: contextPanelWidth, minWidth: contextPanelWidth }}>
          <div className="context-panel-header">
            <div className="tab-row ctx-panel-tab-row">
              <button className={`tab-btn ${ctxTab === "info" ? "" : ""}`} data-on={ctxTab === "info"} onClick={() => setCtxTab("info")}>会话</button>
              <button className={`tab-btn ${ctxTab === "graph" ? "" : ""}`} data-on={ctxTab === "graph"} onClick={() => setCtxTab("graph")}>图谱</button>
              <button className={`tab-btn ${ctxTab === "files" ? "" : ""}`} data-on={ctxTab === "files"} onClick={() => setCtxTab("files")}>文件</button>
              <button className={`tab-btn ${ctxTab === "preview" ? "" : ""}`} data-on={ctxTab === "preview"} onClick={() => setCtxTab("preview")}>预览</button>
            </div>
            <button className="btn-icon btn-sm" onClick={() => setShowContextPanel(false)}><I.x /></button>
          </div>
          {ctxTab === "graph" ? (
            <GraphPanel cwd={activeSession?.cwd} />
          ) : ctxTab === "files" ? (
            <FileTree cwd={activeSession?.cwd || ""} />
          ) : ctxTab === "info" ? (
            <SessionInfoPanel
              title={activeSession.title || activeSession.id}
              status={activeSession.status}
              messageCount={activeSession.messages.length}
              connected={connected}
              inputTokens={cumulativeUsage.input_tokens}
              outputTokens={cumulativeUsage.output_tokens}
              cacheTokens={cumulativeUsage.cache_tokens}
              cost={cumulativeUsage.cost}
              cachePct={activeSession?.cachePct}
            />
          ) : ctxTab === "preview" ? (
            <PreviewPanel defaultUrl={previewUrl} />
          ) : (
            <div style={{flex:1,overflow:"hidden"}}>{/* FileTree placeholder */}
            </div>
          )}
        </div>
        </div>
      )}

      {/* Modals & Overlays */}
      {showNewModal && <NewSessionModal projectName={projectName} setProjectName={setProjectName} cwd={cwd} setCwd={setCwd} prompt={prompt} setPrompt={setPrompt} onClose={() => setShowNewModal(false)} onCreate={handleNewSession} scanning={scanning} scanResult={scanResult} />}
      {showSettings && <SettingsModal onClose={() => setShowSettings(false)} apiKey={cfg.apiKey} model={selectedModel} onSave={handleSaveSettings} activeCwd={activeSession?.cwd} />}
      {showPhoneConnect && <ConnectPhoneModal onClose={() => setShowPhoneConnect(false)} />}
      {showAbout && <AboutModal onClose={() => setShowAbout(false)} />}
      {confirmDelete && (
        confirmDelete.kind === "project" ? (
          <ConfirmDialog
            msg="确定删除该项目所有数据？此操作不可撤销。"
            options={[{ label: "删除项目数据", kind: "danger" as const }]}
            onChoice={confirmDeleteChoice}
            onClose={() => setConfirmDelete(null)}
          />
        ) : (
          <ConfirmDialog
            msg="选择要删除的内容："
            options={[
              { label: "删除对话", kind: "ghost" as const },
              { label: "删除项目数据", kind: "danger" as const },
            ]}
            onChoice={confirmDeleteChoice}
            onClose={() => setConfirmDelete(null)}
          />
        )
      )}
      <CommandPalette open={showCmdPalette} onClose={() => setShowCmdPalette(false)} commands={paletteCommands} />
      <ToastContainer toasts={toasts} onRemove={removeToast} />
    </div>
  );
}

export default App;
