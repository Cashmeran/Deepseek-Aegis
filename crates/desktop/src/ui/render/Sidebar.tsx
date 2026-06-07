// Sidebar — workspace-grouped project list. 1:1 port from DeepSeek-GUI.
import { useState, useMemo, useEffect, useCallback, useRef, type ReactElement, type MouseEvent } from "react";
import { I } from "../icons";

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
  onOpenAbout: () => void;
  connected: boolean; model: string;
  search: string; setSearch: (s: string) => void;
};

function fmtTime(ts?: number): string {
  if (!ts) return "";
  const ms = ts < 1e12 ? ts * 1000 : ts;
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "刚才";
  if (s < 3600) return `${Math.floor(s / 60)}分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)}小时前`;
  return `${Math.floor(s / 86400)}天前`;
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

function folderName(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}

/* ── Context Menu ──────────────────────────────────────────── */

type CtxMenu = {
  x: number; y: number;
  items: { label: string; danger?: boolean; action: () => void }[];
} | null;

function ContextMenu({ menu, onClose }: { menu: CtxMenu; onClose: () => void }) {
  const closeRef = useRef<() => void>(() => {});

  useEffect(() => {
    if (!menu) return;
    const close = () => { onClose(); };
    closeRef.current = close;
    // Delay so the right-click that opens the menu doesn't close it
    const id = setTimeout(() => {
      window.addEventListener("click", close);
      window.addEventListener("contextmenu", close);
    }, 0);
    return () => {
      clearTimeout(id);
      window.removeEventListener("click", close);
      window.removeEventListener("contextmenu", close);
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
  onOpenSettings, onOpenAbout, connected, model, search, setSearch,
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
        { label: "删除对话", action: () => onDelete(s.id) },
        { label: "删除项目数据", danger: true, action: () => onDeleteProject(s.cwd || "") },
      ],
    });
  }, [onRename, onDelete, onDeleteProject]);

  const showGroupMenu = useCallback((e: MouseEvent, workspace: string) => {
    e.preventDefault();
    setCtxMenu({
      x: e.clientX, y: e.clientY,
      items: [
        { label: "删除项目数据", danger: true, action: () => onDeleteProject(workspace) },
      ],
    });
  }, [onDeleteProject]);

  return (
    <aside className={`sidebar ${collapsed ? "collapsed" : ""}`}>
      <div className="sidebar-header">
        {collapsed ? (
          <button className="btn-icon" title="展开" onClick={onToggle} style={{margin:"0 auto"}}><I.chevronRight /></button>
        ) : (
          <>
            <span className="sidebar-logo">Aegis</span>
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
            <div style={{display:"flex",alignItems:"center",gap:6,padding:"2px 8px",borderRadius:8,border:"1px solid var(--border)",background:"var(--bg-app)"}}>
              <I.search />
              <input value={search} onChange={e => setSearch(e.target.value)} placeholder="搜索项目…"
                style={{flex:1,border:"none",outline:"none",background:"transparent",color:"var(--fg-primary)",fontSize:13,padding:"6px 0"}} />
              {search && <button onClick={() => setSearch("")} style={{background:"none",border:"none",cursor:"pointer",color:"var(--fg-muted)",fontSize:16,padding:0,lineHeight:1}}>×</button>}
            </div>
          </div>
          <div className="sidebar-sessions">
            {groups.length === 0 ? (
              <div style={{padding:"16px 10px",textAlign:"center",color:"var(--fg-muted)",fontSize:13,lineHeight:1.6}}>
                {search ? "没有匹配的项目" : "暂无项目\n点击 + 创建"}
              </div>
            ) : groups.map(([workspace, list]) => {
              const isCollapsed = collapsedGroups[workspace] === true;
              const sorted = [...list].sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
              return (
                <div key={workspace} style={{marginBottom:4}}>
                  <button
                    onClick={() => setCollapsedGroups(c => ({...c, [workspace]: !c[workspace]}))}
                    onContextMenu={(e) => showGroupMenu(e, workspace)}
                    style={{display:"flex",alignItems:"center",gap:8,width:"100%",padding:"6px 8px",border:"none",background:"transparent",cursor:"pointer",color:"var(--fg-muted)",fontSize:12.5,fontWeight:500,borderRadius:6}}
                    className="sidebar-action-item"
                  >
                    {isCollapsed ? <I.chevronRight /> : <I.chevronRight />}
                    <I.folder />
                    <span style={{flex:1,textAlign:"left",overflow:"hidden",textOverflow:"ellipsis",whiteSpace:"nowrap"}}>{folderName(workspace)}</span>
                    <span style={{fontSize:11,color:"var(--fg-muted)"}}>{sorted.length}</span>
                  </button>
                  {!isCollapsed && sorted.map(s => (
                    <div key={s.id}
                      onClick={() => onSelect(s.id)}
                      onContextMenu={(e) => showSessionMenu(e, s)}
                      style={{
                        display:"flex",alignItems:"center",gap:8,padding:"5px 10px 5px 28px",
                        cursor:"pointer",borderRadius:6,fontSize:13,
                        background: s.id === activeSessionId ? "var(--bg-active)" : "transparent",
                        color: s.id === activeSessionId ? "var(--fg-primary)" : "var(--fg-secondary)",
                        fontWeight: s.id === activeSessionId ? 600 : 400,
                        transition:"background 120ms ease-out",
                      }}
                      title={s.cwd || s.id}
                    >
                      <span style={{flex:1,overflow:"hidden",textOverflow:"ellipsis",whiteSpace:"nowrap"}}>{s.title || s.id}</span>
                      {s.status === "running" && (
                        <span style={{width:8,height:8,borderRadius:"50%",background:"var(--accent)",flexShrink:0}} className="pulse-dot" />
                      )}
                      <span style={{fontSize:11,color:"var(--fg-muted)",flexShrink:0}}>{fmtTime(s.updatedAt)}</span>
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
          <div className="sidebar-actions-bottom">
            <button className="sidebar-action-item" onClick={onOpenSettings}><I.settings /> 设置</button>
            <button className="sidebar-action-item" onClick={onOpenAbout}><I.info /> 关于</button>
          </div>
          <div className="sidebar-footer">{connected ? "已连接" : "未连接"} · {model || "未配置"}</div>
        </>
      )}
      <ContextMenu menu={ctxMenu} onClose={() => setCtxMenu(null)} />
    </aside>
  );
}
