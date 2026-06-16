// Sidebar — workspace-grouped project list.
import { useState, useMemo, useCallback, useEffect, type ReactElement, type MouseEvent } from "react";
import { I } from "../icons";
import { AegisWordmark } from "./AegisLogo";

type Session = {
  id: string; title: string; status: string; cwd?: string;
  updatedAt?: number; createdAt?: number; hydrated?: boolean;
};

type Props = {
  collapsed: boolean; onToggle: () => void;
  sessions: Record<string, Session>;
  activeSessionId: string | null;
  onSelect: (id: string) => void;
  onDelete: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onDeleteProject: (cwd: string) => void;
  onNew: () => void;
  onOpenSettings: () => void;
  onOpenPhoneConnect: () => void;
  onOpenAbout: () => void;
  connected: boolean; model: string;
  search: string; setSearch: (s: string) => void;
};

function fmtTime(ts?: number): string {
  if (!ts) return "";
  const ms = ts < 1e12 ? ts * 1000 : ts;
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "刚刚";
  if (s < 3600) return `${Math.floor(s / 60)}分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)}小时前`;
  return `${Math.floor(s / 86400)}天前`;
}

function folderName(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}

type Group = [string, Session[]];

function buildGroups(sessions: Record<string, Session>, query: string): Group[] {
  const map = new Map<string, Session[]>();
  const q = query.trim().toLowerCase();
  for (const s of Object.values(sessions)) {
    if (q) {
      const hay = [s.title, s.id, s.cwd ?? ""].join("\n").toLowerCase();
      if (!hay.includes(q)) continue;
    }
    const key = s.cwd || "未分类";
    const arr = map.get(key) ?? [];
    arr.push(s);
    map.set(key, arr);
  }
  return [...map.entries()].sort(([a], [b]) => a.localeCompare(b));
}

/* ── Context Menu ──────────────────────────────────────────── */

type CtxMenu = {
  x: number; y: number;
  items: { label: string; danger?: boolean; action: () => void }[];
} | null;

function ContextMenu({ menu, onClose }: { menu: CtxMenu; onClose: () => void }) {
  // Close on click outside or scroll
  useEffect(() => {
    if (!menu) return;
    const close = () => onClose();
    // Delay to avoid the same right-click that opens the menu from closing it
    const id = setTimeout(() => {
      window.addEventListener("click", close);
      window.addEventListener("contextmenu", close);
      window.addEventListener("scroll", close, { capture: true });
    }, 0);
    return () => {
      clearTimeout(id);
      window.removeEventListener("click", close);
      window.removeEventListener("contextmenu", close);
      window.removeEventListener("scroll", close, { capture: true });
    };
  }, [menu, onClose]);

  if (!menu) return null;

  return (
    <div className="ctx-menu" style={{ left: menu.x, top: menu.y }}>
      {menu.items.map((item, i) => (
        <button
          key={i}
          className={`ctx-menu-item ${item.danger ? "danger" : ""}`}
          onClick={(e) => { e.stopPropagation(); item.action(); onClose(); }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}

/* ── Sidebar ───────────────────────────────────────────────── */

export function Sidebar({
  collapsed, onToggle, sessions, activeSessionId, onSelect, onDelete, onRename, onDeleteProject, onNew,
  onOpenSettings, onOpenPhoneConnect, onOpenAbout, connected, model, search, setSearch,
}: Props): ReactElement {
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({});
  const [ctxMenu, setCtxMenu] = useState<CtxMenu>(null);
  const groups = useMemo(() => buildGroups(sessions, search), [sessions, search]);

  const showSessionMenu = useCallback((e: MouseEvent, s: Session) => {
    e.preventDefault();
    setCtxMenu({
      x: e.clientX, y: e.clientY,
      items: [
        { label: "重命名", action: () => { const title = prompt("新名称", s.title); if (title?.trim()) onRename(s.id, title.trim()); } },
        { label: "删除会话", action: () => onDelete(s.id) },
      ],
    });
  }, [onRename, onDelete]);

  const showGroupMenu = useCallback((e: MouseEvent, workspace: string) => {
    e.preventDefault();
    setCtxMenu({
      x: e.clientX, y: e.clientY,
      items: [
        { label: "新建会话", action: onNew },
        { label: "删除项目数据", danger: true, action: () => onDeleteProject(workspace) },
      ],
    });
  }, [onDeleteProject, onNew]);

  return (
    <aside className={`sidebar ${collapsed ? "collapsed" : ""}`}>
      <div className="sidebar-header">
        {collapsed ? (
          <button className="btn-icon" title="展开" onClick={onToggle}><I.chevronRight /></button>
        ) : (
          <>
            <AegisWordmark size={15} />
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
            <I.search />
            <input
              value={search}
              onChange={e => setSearch(e.target.value)}
              placeholder="搜索项目…"
            />
            {search && (
              <button className="sidebar-search-clear" onClick={() => setSearch("")}>×</button>
            )}
          </div>

          <div className="sidebar-sessions">
            {groups.length === 0 ? (
              <div className="sidebar-empty">
                {search ? "没有匹配的项目" : "暂无项目\n点击 + 创建"}
              </div>
            ) : groups.map(([workspace, list]) => {
              const isCollapsed = collapsedGroups[workspace] === true;
              const sorted = [...list].sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
              return (
                <div key={workspace} className="sidebar-group">
                  <button
                    className="sidebar-group-toggle"
                    onClick={() => setCollapsedGroups(c => ({...c, [workspace]: !c[workspace]}))}
                    onContextMenu={(e) => showGroupMenu(e, workspace)}
                  >
                    <span className="sidebar-group-chevron">{isCollapsed ? "▸" : "▾"}</span>
                    <I.folder />
                    <span className="sidebar-group-name">{folderName(workspace)}</span>
                    <span className="sidebar-group-count">{sorted.length}</span>
                  </button>
                  {!isCollapsed && sorted.map(s => (
                    <div
                      key={s.id}
                      className={`session-item ${s.id === activeSessionId ? "active" : ""}`}
                      onClick={() => onSelect(s.id)}
                      onContextMenu={(e) => showSessionMenu(e, s)}
                      title={s.cwd || s.id}
                    >
                      <div className={`session-item-status ${s.status}`} />
                      <span className="session-item-title">{s.title || s.id}</span>
                      <span className="session-item-time">{fmtTime(s.updatedAt)}</span>
                    </div>
                  ))}
                </div>
              );
            })}
          </div>

          <div className="sidebar-actions-bottom">
            <button className="sidebar-action-item" onClick={onOpenSettings}><I.settings />设置</button>
            <button className="sidebar-action-item" onClick={onOpenPhoneConnect}><I.smartphone />连接手机</button>
            <button className="sidebar-action-item" onClick={onOpenAbout}><I.info />关于</button>
          </div>

          <div className="sidebar-footer">{connected ? "已连接" : "未连接"} · {model || "未配置"}</div>
        </>
      )}

      <ContextMenu menu={ctxMenu} onClose={() => setCtxMenu(null)} />
    </aside>
  );
}
