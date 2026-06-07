import { useEffect, useState, useCallback, useRef, useMemo } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useIPC } from "./hooks/useIPC";
import { useAppStore } from "./store/useAppStore";
import MDContent from "./render/markdown";
import { I } from "./icons";
import { Thread } from "./render/Thread";
import { SettingsModal } from "./components/SettingsModal";
import { FileTree } from "./render/FileTree";
import { AegisLogo, AegisWordmark } from "./render/AegisLogo";

/* ── Types ─────────────────────────────────────────────────── */

type ExecMode = "default" | "chat" | "plan" | "yolo";
type Theme = "light" | "dark";

type SlashCmd = { cmd: string; desc: string; icon: React.ReactNode; run: () => void };
type PopupKind = "slash" | "model" | "command" | "skill" | null;

type RealSkill = { name: string; description: string };

const AVAILABLE_MODELS = ["deepseek-v4-pro", "deepseek-v4-flash", "deepseek-v3.2", "deepseek-r1"];

const MODES: { k: ExecMode; label: string; icon: React.ReactNode; desc: string }[] = [
  { k: "chat",    label: "Chat",    icon: <I.globe />,  desc: "纯对话，不使用工具" },
  { k: "plan",    label: "Plan",    icon: <I.list />,   desc: "先制定计划，审批后执行" },
  { k: "default", label: "Default", icon: <I.edit />,   desc: "完整工具集，高风险操作需审批" },
  { k: "yolo",    label: "YOLO",    icon: <I.zap />,    desc: "完整工具集，全自动执行" },
];

// Predefined slash commands shown as quick-pick chips
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

/* ── Toast ─────────────────────────────────────────────────── */

interface ToastItem { id: number; text: string; kind: "success" | "error" | "info"; }
let toastId = 0;
function ToastContainer({ toasts, onRemove }: { toasts: ToastItem[]; onRemove: (id: number) => void }) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-container" role="status" aria-live="polite">
      {toasts.map(t => (<div key={t.id} className={`toast ${t.kind}`} onClick={() => onRemove(t.id)}>{t.text}</div>))}
    </div>
  );
}

/* ── Confirm Dialog ────────────────────────────────────────── */

function ConfirmDialog({ msg, options, onChoice, onClose }: {
  msg: string;
  options: { label: string; kind: "danger" | "ghost" }[];
  onChoice: (idx: number) => void;
  onClose: () => void;
}) {
  return (
    <div className="confirm-overlay" onClick={onClose}>
      <div className="confirm-box" onClick={e => e.stopPropagation()}>
        <p>{msg}</p>
        <div className="modal-actions">
          <button className="btn btn-ghost btn-sm" onClick={onClose}>取消</button>
          {options.map((opt, i) => (
            <button key={i} className="btn btn-primary btn-sm"
              style={opt.kind === "danger" ? {background:"var(--danger)"} : {}}
              onClick={() => onChoice(i)}>{opt.label}</button>
          ))}
        </div>
      </div>
    </div>
  );
}

/* ── Time formatter ────────────────────────────────────────── */

function relativeTime(ts: number): string {
  // ts may be Unix seconds (from backend) or milliseconds (from Date.now())
  const ms = ts < 1e12 ? ts * 1000 : ts;
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "刚刚";
  if (s < 3600) return `${Math.floor(s / 60)}分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)}小时前`;
  return `${Math.floor(s / 86400)}天前`;
}

/* ── Sidebar ───────────────────────────────────────────────── */

function Sidebar({
  collapsed, onToggle, sessionList, activeSessionId,
  onSelect, onDelete, onRename, onNew, onOpenSettings, onOpenAbout,
  connected, model, search, setSearch,
}: {
  collapsed: boolean; onToggle: () => void;
  sessionList: { id: string; title: string; status: string; updatedAt?: number }[];
  activeSessionId: string | null; onSelect: (id: string) => void;
  onDelete: (id: string) => void; onRename: (id: string, title: string) => void;
  onNew: () => void; onOpenSettings: () => void; onOpenAbout: () => void;
  connected: boolean; model: string;
  search: string; setSearch: (s: string) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const filtered = search ? sessionList.filter(s => s.title.toLowerCase().includes(search.toLowerCase())) : sessionList;
  const startRename = (s: typeof sessionList[0]) => { setEditingId(s.id); setEditValue(s.title || ""); };
  const commitRename = (id: string) => { if (editValue.trim()) onRename(id, editValue.trim()); setEditingId(null); };

  return (
    <aside className={`sidebar ${collapsed ? "collapsed" : ""}`}>
      <div className="sidebar-header">
        {collapsed ? (
          <button className="btn-icon" title="展开" onClick={onToggle} style={{margin:"0 auto"}}><I.chevronRight /></button>
        ) : (
          <>
            <AegisWordmark size={17} />
            <div className="sidebar-actions">
              <button className="btn-icon" title="新建项目" onClick={onNew}><I.plus /></button>
              <button className="btn-icon" title="收起" onClick={onToggle}><I.chevronLeft /></button>
            </div>
          </>
        )}
      </div>
      {!collapsed && (
        <>
          <div className="sidebar-search">
            <input value={search} onChange={e => setSearch(e.target.value)} placeholder="搜索会话…" />
          </div>
          <div className="sidebar-sessions">
            {filtered.map(s => (
              <div key={s.id} className={`session-item ${s.id === activeSessionId ? "active" : ""}`} onClick={() => onSelect(s.id)}>
                <span className={`session-item-status ${s.status}`} />
                {editingId === s.id ? (
                  <input className="session-rename-input" value={editValue} onChange={e => setEditValue(e.target.value)}
                    onBlur={() => commitRename(s.id)} onKeyDown={e => { if (e.key === "Enter") commitRename(s.id); if (e.key === "Escape") setEditingId(null); }}
                    onClick={e => e.stopPropagation()} autoFocus />
                ) : (
                  <>
                    <span className="session-item-title">{s.title || s.id}</span>
                    {s.updatedAt && <span className="session-item-time">{relativeTime(s.updatedAt)}</span>}
                  </>
                )}
                <button className="btn-icon btn-sm" onClick={e => { e.stopPropagation(); startRename(s); }} title="重命名"><I.edit /></button>
                <button className="btn-icon btn-sm" onClick={e => { e.stopPropagation(); onDelete(s.id); }} title="删除"><I.trash /></button>
              </div>
            ))}
            {sessionList.length === 0 && (
              <div style={{fontSize:12,color:'var(--fg-muted)',textAlign:'center',padding:'24px 8px',lineHeight:1.6}}>暂无项目<br/>点击 + 创建</div>
            )}
          </div>
          <div className="sidebar-actions-bottom">
            <button className="sidebar-action-item" onClick={onOpenSettings}><I.settings /> 设置</button>
            <button className="sidebar-action-item" onClick={onOpenAbout}><I.info /> 关于 Aegis</button>
          </div>
          <div className="sidebar-footer">{connected ? "已连接" : "未连接"} · {model || "未配置"}</div>
        </>
      )}
    </aside>
  );
}

/* ── Mode Switch ───────────────────────────────────────────── */

function ModeSwitch({ mode, onChange }: { mode: ExecMode; onChange: (m: ExecMode) => void }) {
  return (
    <div className="mode-switch">
      {MODES.map(m => (
        <button key={m.k} className="mode-seg" data-on={mode === m.k} data-k={m.k}
          onClick={() => onChange(m.k)} title={m.desc}>
          {m.icon}<span>{m.label}</span>
        </button>
      ))}
    </div>
  );
}

/* ── Generic Picker Popup ──────────────────────────────────── */

function PickerPopup({
  items, activeIdx, onPick,
}: {
  items: { id: string; label: string; desc?: string; icon?: React.ReactNode }[];
  activeIdx: number; onPick: (item: { id: string; label: string; desc?: string }) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div className="composer-popup">
      {items.map((c, i) => (
        <div key={c.id} className={`popup-item ${i === activeIdx ? "active" : ""}`}
          onMouseDown={e => { e.preventDefault(); onPick(c); }}>
          {c.icon && <span style={{color:"var(--fg-muted)"}}>{c.icon}</span>}
          <span className="popup-item-label">{c.label}</span>
          {c.desc && <span className="popup-item-desc">{c.desc}</span>}
        </div>
      ))}
    </div>
  );
}

/* ── Model Selector Popup ──────────────────────────────────── */

function ModelPopup({ current, onPick, onClose }: { current: string; onPick: (m: string) => void; onClose: () => void }) {
  const items = AVAILABLE_MODELS.map(m => ({ id: m, label: m, desc: m === current ? "当前" : undefined }));
  return (
    <div className="composer-popup" style={{right:0,left:"auto",width:240}}>
      {items.map(m => (
        <div key={m.id} className={`popup-item ${m.id === current ? "active" : ""}`}
          onClick={() => { onPick(m.id); onClose(); }}>
          <span className="popup-item-label" style={{fontFamily:'"JetBrains Mono",monospace',fontSize:12}}>{m.label}</span>
          {m.id === current && <I.check />}
        </div>
      ))}
    </div>
  );
}

/* ── Command Palette ───────────────────────────────────────── */

function CommandPalette({ open, onClose, commands }: {
  open: boolean; onClose: () => void;
  commands: { id: string; label: string; desc: string; icon: React.ReactNode; run: () => void }[];
}) {
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => { if (open) { setQuery(""); setActiveIdx(0); setTimeout(() => inputRef.current?.focus(), 50); } }, [open]);
  const filtered = query ? commands.filter(c => c.label.toLowerCase().includes(query.toLowerCase()) || c.desc.toLowerCase().includes(query.toLowerCase())) : commands;
  useEffect(() => { setActiveIdx(0); }, [query]);
  if (!open) return null;
  return (
    <div className="cmd-overlay" onClick={onClose}>
      <div className="cmd-palette" onClick={e => e.stopPropagation()}>
        <div className="cmd-input-row"><I.command /><input ref={inputRef} value={query} onChange={e => setQuery(e.target.value)} onKeyDown={e => {
          if (e.key === "ArrowDown") { e.preventDefault(); setActiveIdx(i => Math.min(i + 1, filtered.length - 1)); }
          else if (e.key === "ArrowUp") { e.preventDefault(); setActiveIdx(i => Math.max(i - 1, 0)); }
          else if (e.key === "Enter") { e.preventDefault(); const c = filtered[activeIdx]; if (c) { c.run(); onClose(); } }
          else if (e.key === "Escape") onClose();
        }} placeholder="输入命令…" /></div>
        <div className="cmd-results">
          {filtered.map((c, i) => (
            <div key={c.id} className={`cmd-item ${i === activeIdx ? "active" : ""}`} onClick={() => { c.run(); onClose(); }}>
              <span style={{color:"var(--fg-muted)"}}>{c.icon}</span>
              <span className="cmd-item-label">{c.label}</span>
              <span className="cmd-item-desc">{c.desc}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}


/* ── Composer ──────────────────────────────────────────────── */

function Composer({
  prompt, setPrompt, onSubmit, onStop, isRunning,
  mode, setMode, model, onModelChange, cwd,
  inputRef, slashCommands, skills,
}: {
  prompt: string; setPrompt: (v: string) => void;
  onSubmit: () => void; onStop: () => void; isRunning: boolean;
  mode: ExecMode; setMode: (m: ExecMode) => void;
  model: string; onModelChange: (m: string) => void;
  cwd?: string;
  inputRef: React.RefObject<HTMLTextAreaElement | null>;
  slashCommands: SlashCmd[];
  skills: RealSkill[];
}) {
  const [popup, setPopup] = useState<PopupKind>(null);
  const [slashIdx, setSlashIdx] = useState(0);
  const [slashFiltered, setSlashFiltered] = useState<SlashCmd[]>([]);

  const handleInput = useCallback((val: string) => {
    setPrompt(val);
    const lines = val.split("\n");
    const last = lines[lines.length - 1];
    if (last.startsWith("/") && !last.includes(" ") && last.length >= 2) {
      const query = last.slice(1).toLowerCase();
      setSlashFiltered(slashCommands.filter(c => c.cmd.toLowerCase().includes(query)));
      setSlashIdx(0); setPopup("slash");
    } else {
      if (popup === "slash") setPopup(null);
    }
  }, [setPrompt, slashCommands, popup]);

  const pickSlash = useCallback((c: SlashCmd) => {
    const lines = prompt.split("\n"); lines.pop();
    setPrompt(lines.join("\n")); setPopup(null); c.run();
  }, [prompt, setPrompt]);

  const handleKey = (e: React.KeyboardEvent) => {
    if (popup === "slash") {
      if (e.key === "ArrowDown") { e.preventDefault(); setSlashIdx(i => Math.min(i + 1, slashFiltered.length - 1)); }
      else if (e.key === "ArrowUp") { e.preventDefault(); setSlashIdx(i => Math.max(i - 1, 0)); }
      else if (e.key === "Enter") { e.preventDefault(); if (slashFiltered[slashIdx]) pickSlash(slashFiltered[slashIdx]); }
      else if (e.key === "Escape") setPopup(null);
      else if (e.key === "Tab") { e.preventDefault(); if (slashFiltered.length > 0) pickSlash(slashFiltered[0]); }
      return;
    }
    if (popup === "model" || popup === "command" || popup === "skill") { if (e.key === "Escape") setPopup(null); return; }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); onSubmit(); }
  };

  const handleFilePick = async () => {
    try {
      const files = await openDialog({ multiple: true, title: "选择文件" });
      if (files) {
        const paths = Array.isArray(files) ? files : [files];
        const pathsStr = paths.map(p => `"${p}"`).join(" ");
        setPrompt(p => p ? `${p}\n${pathsStr}` : pathsStr);
      }
    } catch { /* noop */ }
  };

  const commandItems = slashCommands.map(c => ({ id: c.cmd, label: c.cmd, desc: c.desc, icon: c.icon }));
  const skillItems = skills.map(s => ({ id: s.name, label: s.name, desc: s.description }));

  return (
    <div className="composer-bar">
      <div style={{position:"relative"}}>
        <div className="composer-context">
          {cwd && <span className="composer-chip"><I.folder /> {cwd.split(/[\\/]/).pop() || cwd}</span>}
          <button className="composer-chip" onClick={() => setPopup(p => p === "command" ? null : "command")}>
            <I.command /> 命令
          </button>
          <button className="composer-chip" onClick={() => setPopup(p => p === "skill" ? null : "skill")}>
            <I.shield /> Skill
          </button>
        </div>

        <div className="composer-card">
          <div className="composer-main">
            <textarea ref={inputRef} className="composer-input" value={prompt}
              onChange={e => handleInput(e.target.value)} onKeyDown={handleKey}
              placeholder={isRunning ? "Agent 工作中…" : "输入消息 (Enter 发送, / 命令, Shift+Enter 换行)"}
              rows={1} disabled={isRunning}
              onInput={e => { const el = e.currentTarget; el.style.height = "auto"; el.style.height = Math.min(el.scrollHeight, 200) + "px"; }} />
            {isRunning ? (
              <button className="composer-send stop" onClick={onStop} title="停止"><I.stop /></button>
            ) : (
              <button className="composer-send" onClick={onSubmit} disabled={!prompt.trim()} title="发送 (Enter)"><I.send /></button>
            )}
          </div>
          <div className="composer-controls">
            <ModeSwitch mode={mode} onChange={setMode} />
            <div className="composer-model" onClick={() => setPopup(p => p === "model" ? null : "model")}><I.cpu /> {model}</div>
            <button className="composer-chip" onClick={handleFilePick} title="附加文件"><I.file /> 文件</button>
            <span style={{flex:1}} />
            <span style={{fontSize:10,color:"var(--fg-muted)"}}>Enter 发送 · / 命令 · Ctrl+K</span>
          </div>
        </div>

        {popup === "slash" && <PickerPopup items={slashFiltered.map(c => ({ id: c.cmd, label: c.cmd, desc: c.desc, icon: c.icon }))} activeIdx={slashIdx} onPick={({id}) => { const c = slashCommands.find(x => x.cmd === id); if (c) pickSlash(c); }} />}
        {popup === "command" && <PickerPopup items={commandItems} activeIdx={-1} onPick={({id}) => {
          setPopup(null);
          const lines = prompt.split("\n"); lines[lines.length - 1] = id;
          setPrompt(lines.join("\n") + " "); inputRef.current?.focus();
        }} />}
        {popup === "skill" && <PickerPopup items={skillItems} activeIdx={-1} onPick={({id}) => {
          setPopup(null);
          setPrompt(p => p ? `${p}\n[Skill: ${id}] ` : `[Skill: ${id}] `);
          inputRef.current?.focus();
        }} />}
        {popup === "model" && <ModelPopup current={model} onPick={onModelChange} onClose={() => setPopup(null)} />}
      </div>
    </div>
  );
}

function AboutModal({ onClose }: { onClose: () => void }) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        <div className="modal-title">关于 Aegis Desktop</div>
        <div style={{fontSize:13,color:"var(--fg-secondary)",lineHeight:1.7}}>
          <p>Aegis Desktop v0.2.0</p>
          <p>基于 DeepSeek 的 AI 编程助手桌面客户端</p>
          <p style={{marginTop:12,fontSize:12,color:"var(--fg-muted)"}}>
            Powered by <span style={{fontWeight:600,color:"var(--accent-text)"}}>Aegis Engine</span> · Tauri v2 · React 19
          </p>
        </div>
        <div className="modal-actions"><button className="btn btn-ghost" onClick={onClose}>关闭</button></div>
      </div>
    </div>
  );
}

function NewSessionModal({
  projectName, setProjectName, cwd, setCwd, prompt, setPrompt,
  onClose, onCreate, scanning, scanResult,
}: {
  projectName: string; setProjectName: (v: string) => void;
  cwd: string; setCwd: (v: string) => void;
  prompt: string; setPrompt: (v: string) => void;
  onClose: () => void; onCreate: () => void;
  scanning?: boolean;
  scanResult?: { total_files: number; languages: { name: string; files: number }[]; duration_ms: number } | null;
}) {
  const pickDir = async () => {
    try { const dir = await openDialog({ directory: true, multiple: false, title: "选择工作目录" }); if (dir && typeof dir === "string") setCwd(dir); } catch { }
  };
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        <div className="modal-title">新建项目</div>
        <div className="modal-field">
          <label>项目名称</label>
          <input className="input" value={projectName} onChange={e => setProjectName(e.target.value)} placeholder="留空使用目录名" autoFocus disabled={scanning} />
        </div>
        <div className="modal-field">
          <label>工作目录</label>
          <div style={{display:"flex",gap:8}}>
            <input className="input" value={cwd} onChange={e => setCwd(e.target.value)} placeholder="留空使用当前目录" style={{flex:1}} disabled={scanning} />
            <button className="btn btn-ghost" onClick={pickDir} style={{flexShrink:0}} disabled={scanning}><I.folder /> 选择</button>
          </div>
        </div>
        <div className="modal-field">
          <label>描述 <span style={{color:"var(--fg-muted)",fontWeight:400}}>(选填，留空直接创建空项目)</span></label>
          <textarea className="textarea" rows={2} value={prompt} onChange={e => setPrompt(e.target.value)} placeholder="描述你要做什么，留空可创建后再说…" disabled={scanning} />
        </div>
        {scanning && (
          <div style={{padding:"10px 0",display:"flex",alignItems:"center",gap:10}}>
            <div className="dot-pulse" />
            <span style={{fontSize:13,color:"var(--fg-secondary)"}}>正在初始化项目，扫描文件中…</span>
          </div>
        )}
        {scanResult && !scanning && (
          <div style={{padding:"8px 12px",background:"var(--bg-hover)",borderRadius:"var(--radius-sm)",marginBottom:8}}>
            <div style={{fontSize:12,fontWeight:600,marginBottom:4,color:"var(--fg-primary)"}}>
              扫描完成 · {scanResult.total_files} 文件 · {scanResult.duration_ms}ms
            </div>
            <div style={{display:"flex",gap:12,flexWrap:"wrap"}}>
              {scanResult.languages.map(l => (
                <span key={l.name} style={{fontSize:11,color:"var(--fg-muted)"}}>{l.name}: {l.files}</span>
              ))}
            </div>
          </div>
        )}
        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={onClose} disabled={scanning}>取消</button>
          <button className="btn btn-primary" onClick={onCreate} disabled={scanning}>{scanning ? "初始化中…" : "创建项目"}</button>
        </div>
      </div>
    </div>
  );
}

/* ── Status Bar ────────────────────────────────────────────── */

function contextWindow(model: string): number {
  if (model.includes("v3") || model.includes("v4")) return 1_000_000;
  if (model.includes("r1")) return 1_000_000;
  return 1_000_000; // DeepSeek V4/V3/R1 all 1M context
}

function StatusBar({
  inputTokens, outputTokens, cost, cachePct,
  isRunning, model, connected,
}: {
  inputTokens: number; outputTokens: number; cost: number; cachePct?: number;
  isRunning: boolean; model: string; connected: boolean;
}) {
  const ctx = contextWindow(model);
  const totalInput = inputTokens + outputTokens;
  const ctxPct = Math.round((totalInput / ctx) * 100);
  const fmtK = (n: number) => n >= 1000 ? `${(n / 1000).toFixed(1)}K` : `${n}`;

  return (
    <div className="status-bar">
      <div className="status-bar-group">
        <span className="status-bar-label">模型</span>
        <span className="status-bar-value">{model}</span>
      </div>
      <div className="status-bar-divider" />
      <div className="status-bar-group" style={{gap:6,flex:0}}>
        <span className="status-bar-label" style={{flexShrink:0}}>Token</span>
        <div className="status-bar-token-bar">
          <div className="status-bar-token-fill" style={{width:`${Math.min(ctxPct, 100)}%`}} />
        </div>
        <span className="status-bar-value" style={{flexShrink:0}}>
          {fmtK(totalInput)}/{fmtK(ctx)} ({ctxPct}%)
        </span>
      </div>
      <div className="status-bar-divider" />
      <div className="status-bar-group">
        <span className="status-bar-label">缓存命中</span>
        <span className={`status-bar-value ${cachePct && cachePct > 50 ? "highlight" : ""}`}>
          {cachePct ?? "--"}%
        </span>
      </div>
      <div className="status-bar-divider" />
      <div className="status-bar-group">
        <span className="status-bar-label">费用</span>
        <span className="status-bar-value">¥{cost.toFixed(4)}</span>
      </div>
      <span style={{flex:1}} />
      <div className="status-bar-group">
        <span className={`status-bar-value ${isRunning ? "highlight" : ""}`}>
          {isRunning ? "运行中" : connected ? "就绪" : "离线"}
        </span>
      </div>
    </div>
  );
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

  const sessions = useAppStore(s => s.sessions);
  const activeSessionId = useAppStore(s => s.activeSessionId);
  const setActiveSessionId = useAppStore(s => s.setActiveSessionId);
  const loadProjectSessions = useAppStore(s => s.loadProjectSessions);

  const handleSelectSession = useCallback(async (id: string) => {
    setActiveSessionId(id);
    const session = sessions[id];
    // Load project session data if this is a historical project (not hydrated yet)
    if (session && !session.hydrated && session.cwd) {
      await loadProjectSessions(session.cwd);
    }
  }, [setActiveSessionId, sessions, loadProjectSessions]);
  const providerConfigs = useAppStore(s => s.providerConfigs);
  const setProviderConfig = useAppStore(s => s.setProviderConfig);
  const globalError = useAppStore(s => s.globalError);
  const setGlobalError = useAppStore(s => s.setGlobalError);

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [showNewModal, setShowNewModal] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [showCmdPalette, setShowCmdPalette] = useState(false);
  const [showContextPanel, setShowContextPanel] = useState(false);
  const [ctxTab, setCtxTab] = useState<"info" | "files">("info");
  const [cwd, setCwd] = useState("");
  const [projectName, setProjectName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [editMode, setEditMode] = useState<ExecMode>("default");
  const [sidebarSearch, setSidebarSearch] = useState("");
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [confirmDelete, setConfirmDelete] = useState<{ id: string; cwd?: string } | null>(null);
  const [selectedModel, setSelectedModel] = useState(providerConfigs.deepseek.model);
  const [loadedSkills, setLoadedSkills] = useState<RealSkill[]>([]);
  const [scanning, setScanning] = useState(false);

  const initProject = useAppStore(s => s.initProject);
  const scanProject = useAppStore(s => s.scanProject);
  const checkProject = useAppStore(s => s.checkProject);
  const projectMeta = useAppStore(s => s.projectMeta);
  const scanResult = useAppStore(s => s.scanResult);

  const addToast = useCallback((text: string, kind: ToastItem["kind"] = "info") => {
    const id = ++toastId; setToasts(t => [...t, { id, text, kind }]);
    setTimeout(() => setToasts(t => t.filter(x => x.id !== id)), 3000);
  }, []);
  const removeToast = useCallback((id: number) => { setToasts(t => t.filter(x => x.id !== id)); }, []);

  const activeSession = activeSessionId ? sessions[activeSessionId] : null;
  const isRunning = activeSession?.status === "running";
  const sessionList = Object.values(sessions).sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
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

  // Smart scroll: only auto-scroll when user is at bottom. Scroll up = detach.
  useEffect(() => {
    if (!messagesAreaRef.current || !isAtBottomRef.current) return;
    messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
  }, [activeSession?.messages]);

  // Track scroll position
  const handleMessagesScroll = useCallback(() => {
    const el = messagesAreaRef.current;
    if (!el) return;
    const threshold = 40; // pixels from bottom considered "at bottom"
    isAtBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
  }, []);

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

  const handleNewSession = useCallback(async () => {
    if (!cfg.apiKey) { addToast("请先在设置中配置 API Key", "error"); return; }
    const name = projectName.trim() || (cwd ? cwd.split(/[\\/]/).pop() || cwd : "新项目");
    const workDir = cwd || ".";

    setScanning(true);

    // If .aegis/ already exists, just open it — don't create duplicate session
    const exists = await checkProject(workDir);
    if (exists) {
      await loadProjectSessions(workDir);
      setActiveSessionId(workDir);
      setScanning(false);
      setShowNewModal(false); setProjectName(""); setPrompt("");
      addToast("已打开现有项目", "success");
      return;
    }

    // New project: init + scan + start session
    const meta = await initProject(workDir);
    if (meta) {
      addToast(`项目已创建 · ${meta.language || "unknown"} · ${meta.file_count} 文件`, "success");
    }
    const result = await scanProject(workDir);
    if (result) {
      addToast(`扫描完成 · ${result.total_files} 文件 · ${result.duration_ms}ms`, "info");
    }
    setScanning(false);

    sendEvent({ type: "session.start", payload: { title: name, prompt: prompt.trim(), cwd: workDir || undefined, provider: "deepseek", apiKey: cfg.apiKey, model: selectedModel, executionMode: editMode } });
    setShowNewModal(false); setProjectName(""); setPrompt("");
  }, [sendEvent, projectName, prompt, cwd, cfg, selectedModel, editMode, addToast, initProject, scanProject, checkProject, loadProjectSessions, setActiveSessionId]);

  const handleContinue = useCallback(() => {
    if (!prompt.trim() || !activeSessionId || isRunning) return;
    const session = sessions[activeSessionId];
    sendEvent({ type: "session.continue", payload: { sessionId: activeSessionId, prompt: prompt.trim(), messages: session?.messages ?? [] } });
    setPrompt(""); inputRef.current?.focus();
  }, [sendEvent, prompt, activeSessionId, isRunning, sessions]);

  const handleStop = useCallback(() => { if (activeSessionId) { sendEvent({ type: "session.stop", payload: { sessionId: activeSessionId } }); addToast("已停止"); } }, [sendEvent, activeSessionId, addToast]);
  const handleDeleteSession = useCallback((id: string) => {
    const session = sessions[id];
    setConfirmDelete({ id, cwd: session?.cwd });
  }, [sessions]);

  const confirmDeleteChoice = useCallback(async (idx: number) => {
    if (!confirmDelete) return;
    if (idx === 0) {
      // Delete conversation only
      const cwd = confirmDelete.cwd;
      if (cwd) {
        try { await window.__TAURI__?.core?.invoke("delete_session", { cwd, sessionId: confirmDelete.id }); } catch {}
      }
      const next = { ...sessions }; delete next[confirmDelete.id];
      useAppStore.setState({ sessions: next });
      if (activeSessionId === confirmDelete.id) setActiveSessionId(null);
      addToast("已删除对话");
    } else if (idx === 1) {
      // Delete project (entire .aegis/ dir)
      const cwd = confirmDelete.cwd;
      if (cwd) {
        try { await window.__TAURI__?.core?.invoke("delete_project", { cwd }); } catch {}
      }
      const next = { ...sessions }; delete next[confirmDelete.id];
      useAppStore.setState({ sessions: next });
      if (activeSessionId === confirmDelete.id) setActiveSessionId(null);
      addToast("已删除项目数据");
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

  // Slash commands — sourced from Aegis CLI catalog (crates/cli/src/app/slash/catalog.rs).
  // Filtered to commands the desktop backend can actually execute.
  const slashCommands = useMemo((): SlashCmd[] => [
    { cmd: "/cancel",     desc: "取消当前正在执行的 Agent 任务",  icon: <I.stop />,    run: () => { if (isRunning) handleStop(); } },
    { cmd: "/compact",    desc: "压缩会话上下文以释放 token",    icon: <I.copy />,     run: () => addToast("compact 暂未实现") },
    { cmd: "/config",     desc: "打开设置面板",                 icon: <I.settings />, run: () => setShowSettings(true) },
    { cmd: "/help",       desc: "关于 Aegis",                  icon: <I.info />,     run: () => setShowAbout(true) },
    { cmd: "/status",     desc: "查看当前会话状态与用量",        icon: <I.panel />,   run: () => setShowContextPanel(p => !p) },
    { cmd: "/usage",      desc: "查看 Token 用量和费用",        icon: <I.cpu />,     run: () => setShowContextPanel(p => !p) },
    { cmd: "/mode",       desc: "切换执行模式 (chat/plan/default/yolo)", icon: <I.list />, run: () => setShowCmdPalette(true) },
    { cmd: "/model",      desc: "切换当前使用的模型",            icon: <I.cpu />,     run: () => setShowCmdPalette(true) },
    { cmd: "/new-session",desc: "创建新的 Agent 会话",          icon: <I.plus />,    run: () => setShowNewModal(true) },
    { cmd: "/resume",     desc: "恢复之前的会话",               icon: <I.folder />,   run: () => setShowCmdPalette(true) },
    { cmd: "/clear",      desc: "清空当前输入",                 icon: <I.x />,       run: () => { setPrompt(""); addToast("已清空"); } },
  ], [isRunning, handleStop, toggleTheme, setPrompt, addToast]);

  const paletteCommands = useMemo(() => {
    const map = slashCommands.reduce((acc, c) => { acc[c.cmd.slice(1)] = c; return acc; }, {} as Record<string, SlashCmd>);
    return Object.values(map).map(c => ({ id: c.cmd.slice(1), label: c.cmd, desc: c.desc, icon: c.icon, run: c.run }));
  }, [slashCommands]);

  return (
    <div className="app-shell">
      <Sidebar collapsed={sidebarCollapsed} onToggle={() => setSidebarCollapsed(!sidebarCollapsed)}
        sessionList={sessionList} activeSessionId={activeSessionId}
        onSelect={handleSelectSession} onDelete={handleDeleteSession} onRename={handleRenameSession}
        onNew={() => setShowNewModal(true)} onOpenSettings={() => setShowSettings(true)} onOpenAbout={() => setShowAbout(true)}
        connected={connected} model={cfg.apiKey ? selectedModel : "未配置 Key"} search={sidebarSearch} setSearch={setSidebarSearch} />
      <div style={{flex:1,display:"flex",flexDirection:"column",minWidth:0}}>
        <div className="main-area">
        <div className="top-bar">
          {activeSession && <span className="top-bar-left">{activeSession.title || activeSession.id}</span>}
          <button className="top-bar-action" onClick={() => setShowCmdPalette(true)} title="命令面板"><I.command /> Ctrl+K</button>
          <button className="top-bar-action" onClick={() => setShowContextPanel(p => !p)} title="上下文面板"><I.panel /></button>
          <button className="top-bar-action" onClick={toggleTheme} title={theme === "light" ? "深色" : "亮色"}>{theme === "light" ? <I.moon /> : <I.sun />}</button>
        </div>
        <div className="messages-area" ref={messagesAreaRef} onScroll={handleMessagesScroll}>
          {globalError && <div style={{maxWidth:800,margin:'0 auto',padding:'0 32px'}}><div className="error-banner"><span>{globalError}</span><button className="btn btn-ghost btn-sm" onClick={() => setGlobalError(null)}>Dismiss</button></div></div>}
          {activeSession ? <Thread messages={activeSession.messages} isRunning={isRunning} /> : (
            <div className="empty-state">
              <div className="empty-logo"><AegisLogo size={64} /></div>
              <div className="empty-title">Aegis Desktop</div>
              <div className="empty-desc">{connected ? "后端已连接" : "后端未连接"} · {cfg.apiKey ? `模型: ${selectedModel}` : "按 Ctrl+K 配置 API Key"}</div>
              <button className="btn btn-primary" onClick={() => setShowNewModal(true)} style={{marginTop:8}}>+ 新建项目</button>
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>
        {activeSession && (
          <Composer prompt={prompt} setPrompt={setPrompt} onSubmit={handleContinue} onStop={handleStop}
            isRunning={isRunning} mode={editMode} setMode={setEditMode} model={selectedModel}
            onModelChange={handleModelChange} cwd={activeSession.cwd} inputRef={inputRef}
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

      {showContextPanel && activeSession && (
        <div className="context-panel">
          <div className="context-panel-header">
            <div style={{display:"flex",gap:4}}>
              <button onClick={() => setCtxTab("info")} style={{padding:"2px 8px",border:"none",borderRadius:4,background:ctxTab==="info"?"var(--bg-hover)":"transparent",color:ctxTab==="info"?"var(--fg-primary)":"var(--fg-muted)",fontSize:12,cursor:"pointer"}}>会话</button>
              <button onClick={() => setCtxTab("files")} style={{padding:"2px 8px",border:"none",borderRadius:4,background:ctxTab==="files"?"var(--bg-hover)":"transparent",color:ctxTab==="files"?"var(--fg-primary)":"var(--fg-muted)",fontSize:12,cursor:"pointer"}}>文件</button>
            </div>
            <button className="btn-icon btn-sm" onClick={() => setShowContextPanel(false)}><I.x /></button>
          </div>
          {ctxTab === "info" ? (<>
            <div className="context-panel-section"><h4>会话</h4><p>{activeSession.title || activeSession.id}</p><p>状态: {activeSession.status} · 模式: {editMode}</p><p>消息: {activeSession.messages.length}</p></div>
            <div className="context-panel-section"><h4>模型</h4><p>{selectedModel}</p><p>Key: {cfg.apiKey ? "已配置" : "未配置"}</p></div>
            <div className="context-panel-section"><h4>连接</h4><p>{connected ? "已连接" : "未连接"}</p></div>
          </>) : (
            <div style={{flex:1,overflow:"hidden"}}>
              <FileTree cwd={activeSession.cwd || ""}
                onSelect={(path) => console.log("Selected:", path)}
                onDoubleClick={(path) => {
                  if (activeSession.cwd) {
                    window.__TAURI__?.core?.invoke("open_mcp_config_dir", { cwd: activeSession.cwd + "/" + path }).catch(() => {});
                  }
                }} />
            </div>
          )}
        </div>
      )}
      {showNewModal && <NewSessionModal projectName={projectName} setProjectName={setProjectName} cwd={cwd} setCwd={setCwd} prompt={prompt} setPrompt={setPrompt} onClose={() => setShowNewModal(false)} onCreate={handleNewSession} scanning={scanning} scanResult={scanResult} />}
      {showSettings && <SettingsModal onClose={() => setShowSettings(false)} apiKey={cfg.apiKey} model={selectedModel} onSave={handleSaveSettings} activeCwd={activeSession?.cwd} />}
      {showAbout && <AboutModal onClose={() => setShowAbout(false)} />}
      {confirmDelete && <ConfirmDialog
        msg="选择要删除的内容："
        options={[
          { label: "删除对话", kind: "ghost" as const },
          { label: "删除项目数据", kind: "danger" as const },
        ]}
        onChoice={confirmDeleteChoice}
        onClose={() => setConfirmDelete(null)}
      />}
      <CommandPalette open={showCmdPalette} onClose={() => setShowCmdPalette(false)} commands={paletteCommands} />
      <ToastContainer toasts={toasts} onRemove={removeToast} />
    </div>
  );
}

export default App;
