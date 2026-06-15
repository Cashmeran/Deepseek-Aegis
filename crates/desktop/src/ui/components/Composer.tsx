// Composer — message input with slash commands, model + reasoning picker, file attach, skill picker
import { useState, useCallback, useRef, useEffect, useMemo, type ReactElement, type KeyboardEvent } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { I } from "../icons";

/* ── Types ─────────────────────────────────────────────────── */

type SlashCmd = { cmd: string; desc: string; icon: React.ReactNode; run: () => void };
type PopupKind = "slash" | "model" | "command" | "skill" | "reasoning" | null;
type RealSkill = { name: string; description: string };

const AVAILABLE_MODELS = ["deepseek-v4-pro", "deepseek-v4-flash"];

const REASONING_OPTIONS: { id: string; label: string }[] = [
  { id: "off", label: "off" },
  { id: "high", label: "high" },
  { id: "max", label: "max" },
];

/* ── Short model name ─────────────────────────────────────── */

function shortModelName(full: string): string {
  const map: Record<string, string> = {
    "deepseek-v4-pro": "V4 Pro",
    "deepseek-v4-flash": "V4 Flash",
  };
  return map[full] ?? full;
}

function reasoningLabel(effort: string): string {
  return REASONING_OPTIONS.find(o => o.id === effort)?.label ?? effort;
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
          {c.icon && <span className="text-muted">{c.icon}</span>}
          <span className="popup-item-label">{c.label}</span>
          {c.desc && <span className="popup-item-desc">{c.desc}</span>}
        </div>
      ))}
    </div>
  );
}

/* ── Model + Reasoning selector ────────────────────────────── */

function ModelReasoningPicker({
  model, reasoningEffort, onModelChange, onReasoningChange,
}: {
  model: string; reasoningEffort: string;
  onModelChange: (m: string) => void; onReasoningChange: (e: string) => void;
}): ReactElement {
  const [popup, setPopup] = useState<"model" | "reasoning" | null>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const [popupPos, setPopupPos] = useState<{ bottom: number; right: number }>({ bottom: 0, right: 0 });

  const updatePos = useCallback(() => {
    if (pickerRef.current) {
      const rect = pickerRef.current.getBoundingClientRect();
      setPopupPos({
        bottom: window.innerHeight - rect.top + 6,
        right: window.innerWidth - rect.right,
      });
    }
  }, []);

  useEffect(() => {
    if (popup) {
      updatePos();
      window.addEventListener("resize", updatePos);
      window.addEventListener("scroll", updatePos, true);
    }
    return () => {
      window.removeEventListener("resize", updatePos);
      window.removeEventListener("scroll", updatePos, true);
    };
  }, [popup, updatePos]);

  // Close on outside click
  useEffect(() => {
    if (!popup) return;
    const onDown = (e: PointerEvent) => {
      if (e.target instanceof Node && !pickerRef.current?.contains(e.target)) {
        setPopup(null);
      }
    };
    window.addEventListener("pointerdown", onDown);
    return () => window.removeEventListener("pointerdown", onDown);
  }, [popup]);

  const modelItems = AVAILABLE_MODELS.map(m => ({
    id: m, label: shortModelName(m), desc: m === model ? "当前" : undefined,
  }));
  const reasoningItems = REASONING_OPTIONS.map(o => ({
    id: o.id, label: o.label, desc: o.id === reasoningEffort ? "当前" : undefined,
  }));

  const popupStyle = {
    bottom: `${popupPos.bottom}px`,
    right: `${popupPos.right}px`,
  };

  return (
    <div className="model-reasoning-picker" ref={pickerRef}>
      <button
        className="composer-model-btn"
        onClick={() => setPopup(p => p === "model" ? null : "model")}
      >
        <span>{shortModelName(model)}</span>
        <I.chevronRight className="chevron-flip" />
      </button>
      <span className="model-reasoning-sep">·</span>
      <button
        className="composer-model-btn reasoning"
        onClick={() => setPopup(p => p === "reasoning" ? null : "reasoning")}
        title={`推理强度: ${reasoningLabel(reasoningEffort)}`}
      >
        <span>{reasoningLabel(reasoningEffort)}</span>
      </button>

      {popup === "model" && (
        <div className="model-reasoning-popup" style={{...popupStyle,minWidth:160}}>
          {modelItems.map(m => (
            <div key={m.id}
              className={`popup-item ${m.id === model ? "active" : ""}`}
              onClick={() => { onModelChange(m.id); setPopup(null); }}>
              <span className="popup-item-label" style={{fontFamily:'"JetBrains Mono",monospace',fontSize:12}}>
                {m.label}
              </span>
              {m.id === model && <I.check />}
            </div>
          ))}
        </div>
      )}
      {popup === "reasoning" && (
        <div className="model-reasoning-popup" style={{...popupStyle,minWidth:120}}>
          {reasoningItems.map(o => (
            <div key={o.id}
              className={`popup-item ${o.id === reasoningEffort ? "active" : ""}`}
              onClick={() => { onReasoningChange(o.id); setPopup(null); }}>
              <span className="popup-item-label">{o.label}</span>
              {o.id === reasoningEffort && <I.check />}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/* ── Composer ──────────────────────────────────────────────── */

export function Composer({
  prompt, setPrompt, onSubmit, onStop, isRunning,
  model, onModelChange, reasoningEffort, onReasoningChange, cwd,
  inputRef, slashCommands, skills,
}: {
  prompt: string; setPrompt: (v: string) => void;
  onSubmit: (text: string) => void; onStop: () => void; isRunning: boolean;
  model: string; onModelChange: (m: string) => void;
  reasoningEffort: string; onReasoningChange: (e: string) => void;
  cwd?: string;
  inputRef: React.RefObject<HTMLTextAreaElement | null>;
  slashCommands: SlashCmd[];
  skills: RealSkill[];
}): ReactElement {
  const [popup, setPopup] = useState<PopupKind>(null);
  const [slashIdx, setSlashIdx] = useState(0);
  const [slashFiltered, setSlashFiltered] = useState<SlashCmd[]>([]);
  const [draft, setDraft] = useState(""); // local draft, sync to parent only on submit
  const [gitBranch, setGitBranch] = useState<string | null>(null);
  const rafRef = useRef(0);

  // Read git branch from .git/HEAD
  useEffect(() => {
    if (!cwd) return;
    const read = async () => {
      try {
        const headPath = `${cwd}/.git/HEAD`.replace(/\\/g, "/");
        const resp = await fetch(`file://${headPath}`);
        const text = await resp.text();
        const match = text.match(/ref: refs\/heads\/(.+)/);
        setGitBranch(match ? match[1] : text.trim().slice(0, 7));
      } catch { setGitBranch(null); }
    };
    read();
  }, [cwd]);

  // Sync parent prompt → local draft when parent changes externally
  useEffect(() => { setDraft(prompt); }, [prompt]);

  const handleInput = useCallback((val: string) => {
    setDraft(val);
    const lines = val.split("\n");
    const last = lines[lines.length - 1];
    if (last.startsWith("/") && last.length >= 2) {
      // CC-style: /command args — extract command name before first space
      const cmdName = last.split(" ")[0].slice(1).toLowerCase();
      const query = cmdName;
      setSlashFiltered(slashCommands.filter(c => c.cmd.toLowerCase().includes(query)));
      setSlashIdx(0); setPopup("slash");
    } else {
      if (popup === "slash") setPopup(null);
    }
  }, [slashCommands, popup]);

  // Auto-resize textarea via rAF — avoid sync layout thrashing
  const autoResize = useCallback(() => {
    if (rafRef.current) cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      const el = inputRef.current;
      if (el) {
        el.style.height = "auto";
        el.style.height = Math.min(el.scrollHeight, 200) + "px";
      }
    });
  }, [inputRef]);

  useEffect(() => { autoResize(); }, [draft, autoResize]);

  const pickSlash = useCallback((c: SlashCmd) => {
    const lines = draft.split("\n"); lines.pop();
    const next = lines.join("\n");
    setDraft(next); setPopup(null); c.run();
  }, [draft]);

  const handleSubmit = useCallback(() => {
    const v = draft.trim();
    if (!v) return;
    setDraft("");
    onSubmit(v);  // pass text directly, parent handles send
    inputRef.current?.focus();
  }, [draft, onSubmit, inputRef]);

  const handleKey = (e: KeyboardEvent) => {
    if (popup === "slash") {
      if (e.key === "ArrowDown") { e.preventDefault(); setSlashIdx(i => Math.min(i + 1, slashFiltered.length - 1)); }
      else if (e.key === "ArrowUp") { e.preventDefault(); setSlashIdx(i => Math.max(i - 1, 0)); }
      else if (e.key === "Enter") { e.preventDefault(); if (slashFiltered[slashIdx]) pickSlash(slashFiltered[slashIdx]); }
      else if (e.key === "Escape") setPopup(null);
      else if (e.key === "Tab") { e.preventDefault(); if (slashFiltered.length > 0) pickSlash(slashFiltered[0]); }
      return;
    }
    if (popup === "model" || popup === "command" || popup === "skill" || popup === "reasoning") {
      if (e.key === "Escape") { setPopup(null); return; }
      if (e.key === "Enter") {
        if (!e.shiftKey) { setPopup(null); e.preventDefault(); handleSubmit(); }
        // Shift+Enter: close popup, let textarea insert newline
        else { setPopup(null); }
        return;
      }
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      // CC-style: if draft is a slash command, execute it directly
      const lastLine = draft.split("\n").pop() || "";
      if (lastLine.startsWith("/")) {
        const parts = lastLine.split(" ");
        const cmdName = parts[0];
        const matched = slashCommands.find(c => c.cmd === cmdName);
        if (matched) {
          if (cmdName === "/goal" && parts.length > 1) {
            // /goal objective | criteria → parse and send as goal
            setPopup(null);
            const rest = parts.slice(1).join(" ");
            const pipeIdx = rest.indexOf("|");
            const objective = pipeIdx >= 0 ? rest.slice(0, pipeIdx).trim() : rest.trim();
            const criteria = pipeIdx >= 0 ? rest.slice(pipeIdx + 1).trim() : undefined;
            setDraft("");
            (window as any).__aegisGoal = { objective, criteria };
            handleSubmit();
            return;
          }
          pickSlash(matched);
          return;
        }
      }
      handleSubmit();
    }
  };

  const handleFilePick = async () => {
    try {
      const files = await openDialog({ multiple: true, title: "选择文件" });
      if (files) {
        const paths = Array.isArray(files) ? files : [files];
        const pathsStr = paths.map(p => `"${p}"`).join(" ");
        setDraft(p => p ? `${p}\n${pathsStr}` : pathsStr);
      }
    } catch { /* noop */ }
  };

  const commandItems = useMemo(() => slashCommands.map(c => ({ id: c.cmd, label: c.cmd, desc: c.desc, icon: c.icon })), [slashCommands]);
  const skillItems = useMemo(() => skills.map(s => ({ id: s.name, label: s.name, desc: s.description })), [skills]);

  return (
    <div className="composer-bar">
      <div className="composer-bar-inner">
        <div className="composer-card">
          <div className="composer-main">
            <textarea ref={inputRef} className="composer-input" value={draft}
              onChange={e => handleInput(e.target.value)} onKeyDown={handleKey}
              placeholder={isRunning ? "Agent 工作中…" : "输入消息 (Enter 发送, / 命令, Shift+Enter 换行)"}
              rows={1} disabled={isRunning} />
            {isRunning ? (
              <button className="composer-send stop" onClick={onStop} title="停止"><I.stop /></button>
            ) : (
              <button className="composer-send" onClick={handleSubmit} disabled={!draft.trim()} title="发送 (Enter)"><I.send /></button>
            )}
          </div>
          <div className="composer-controls">
            <div className="composer-controls-left">
              {cwd && <span className="composer-chip"><I.folder />{cwd.split(/[\\/]/).pop() || cwd}</span>}
              {gitBranch && <span className="git-branch-chip">{gitBranch}</span>}
              <button className="composer-chip" onClick={() => setPopup(p => p === "command" ? null : "command")}>
                <I.command />命令
              </button>
              <button className="composer-chip" onClick={() => setPopup(p => p === "skill" ? null : "skill")}>
                <I.shield />Skill
              </button>
              <button className="composer-chip" onClick={handleFilePick} title="附加文件">
                <I.file />文件
              </button>
            </div>
            <span className="spacer" />
            <ModelReasoningPicker
              model={model}
              reasoningEffort={reasoningEffort}
              onModelChange={onModelChange}
              onReasoningChange={onReasoningChange}
            />
          </div>
        </div>

        {popup === "slash" && <PickerPopup items={slashFiltered.map(c => ({ id: c.cmd, label: c.cmd, desc: c.desc, icon: c.icon }))} activeIdx={slashIdx} onPick={({id}) => { const c = slashCommands.find(x => x.cmd === id); if (c) pickSlash(c); }} />}
        {popup === "command" && <PickerPopup items={commandItems} activeIdx={-1} onPick={({id}) => {
          setPopup(null);
          // Execute command directly — don't insert text
          const cmd = slashCommands.find(c => c.cmd === id);
          if (cmd) { setDraft(""); cmd.run(); inputRef.current?.focus(); }
        }} />}
        {popup === "skill" && (
          skillItems.length === 0 ? (
            <div className="composer-popup">
              <div className="popup-item" style={{ color: "var(--fg-muted)", justifyContent: "center", padding: "14px" }}>
                当前无可用 Skill
              </div>
            </div>
          ) : (
            <PickerPopup items={skillItems} activeIdx={-1} onPick={({id}) => {
              setPopup(null);
              setDraft(p => p ? `${p}\n[Skill: ${id}] ` : `[Skill: ${id}] `);
              inputRef.current?.focus();
            }} />
          )
        )}
      </div>
    </div>
  );
}
