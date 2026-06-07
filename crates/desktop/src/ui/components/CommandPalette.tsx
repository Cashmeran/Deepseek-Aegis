// CommandPalette — Cmd+K overlay with fuzzy search
import { useEffect, useState, useRef, type ReactElement } from "react";
import { I } from "../icons";

export function CommandPalette({ open, onClose, commands }: {
  open: boolean; onClose: () => void;
  commands: { id: string; label: string; desc: string; icon: React.ReactNode; run: () => void }[];
}): ReactElement | null {
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
