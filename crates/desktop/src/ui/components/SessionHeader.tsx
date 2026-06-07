// SessionHeader — glass topbar with editable title, meta row, pill action buttons
import { useState, type ReactElement, type KeyboardEvent } from "react";
import { I } from "../icons";

function folderName(path?: string): string {
  if (!path) return "";
  return path.split(/[/\\]/).pop() || path;
}

function fmtTime(ts?: number): string {
  if (!ts) return "";
  const ms = ts < 1e12 ? ts * 1000 : ts;
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "刚刚";
  if (s < 3600) return `${Math.floor(s / 60)}分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)}小时前`;
  return `${Math.floor(s / 86400)}天前`;
}

export function SessionHeader({
  title, cwd, isRunning, status, updatedAt,
  onRename, onToggleContextPanel, onToggleTheme, onOpenCmdPalette,
  isDark,
}: {
  title: string; cwd?: string; model: string; isRunning: boolean;
  status: string; updatedAt?: number;
  onRename: (title: string) => void;
  onToggleContextPanel: () => void;
  onToggleTheme: () => void;
  onOpenCmdPalette: () => void;
  isDark: boolean;
}): ReactElement {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);

  const commitTitle = () => {
    const next = draft.trim();
    if (next && next !== title) onRename(next);
    else setDraft(title);
    setEditing(false);
  };

  const handleKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") { e.preventDefault(); commitTitle(); }
    if (e.key === "Escape") { setDraft(title); setEditing(false); }
  };

  return (
    <div className="session-header">
      <div className="session-header-left">
        {editing ? (
          <input
            className="session-header-title-input"
            value={draft}
            onChange={e => setDraft(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={handleKey}
            autoFocus
          />
        ) : (
          <button
            className="session-header-title"
            onClick={() => { setDraft(title); setEditing(true); }}
            title="点击编辑标题"
          >
            {title}
          </button>
        )}
        <div className="session-header-meta">
          {cwd && <span className="session-header-meta-item"><I.folder />{folderName(cwd)}</span>}
          <span className="session-header-meta-sep">·</span>
          <span className="session-header-meta-item">{fmtTime(updatedAt)}</span>
          {isRunning && (
            <>
              <span className="session-header-meta-sep">·</span>
              <span className="session-header-badge running">运行中</span>
            </>
          )}
        </div>
      </div>

      <div className="session-header-actions">
        <button className="pill-btn" onClick={onToggleContextPanel} title="上下文面板">
          <I.panel />
        </button>
        <button className="pill-btn" onClick={onToggleTheme} title={isDark ? "亮色模式" : "深色模式"}>
          {isDark ? <I.sun /> : <I.moon />}
        </button>
      </div>
    </div>
  );
}
