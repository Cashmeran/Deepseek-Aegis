import { useState, useEffect, useCallback, type ReactElement } from "react";

type FileNode = { name: string; path: string; isDir: boolean; children?: FileNode[] };

function buildTree(paths: string[]): FileNode[] {
  const root: FileNode[] = [];
  for (const p of paths) {
    const parts = p.replace(/\\/g, "/").split("/");
    let current = root;
    for (let i = 0; i < parts.length; i++) {
      const name = parts[i];
      if (!name) continue;
      const isLast = i === parts.length - 1;
      let node = current.find(n => n.name === name);
      if (!node) {
        node = { name, path: parts.slice(0, i + 1).join("/"), isDir: !isLast, children: isLast ? undefined : [] };
        current.push(node);
      }
      if (!isLast && node.children) current = node.children;
    }
  }
  // Sort: dirs first, then alphabetically
  const sort = (nodes: FileNode[]) => {
    nodes.sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    for (const n of nodes) if (n.children) sort(n.children);
  };
  sort(root);
  return root;
}

const EXT_ICONS: Record<string, string> = {
  rs: "🦀", py: "🐍", ts: "🔷", tsx: "⚛️", js: "🟨", jsx: "⚛️",
  json: "📋", toml: "⚙️", md: "📝", css: "🎨", html: "🌐",
  svg: "🖼️", png: "🖼️", jpg: "🖼️", gitignore: "🙈", lock: "🔒",
  rs: "🦀", go: "🔵", java: "☕", cpp: "⚡", c: "⚡", h: "📌",
  yaml: "⚙️", yml: "⚙️", dockerfile: "🐳", sh: "💻", bat: "💻",
};

function getIcon(name: string, isDir: boolean): string {
  if (isDir) return "📁";
  const ext = name.split(".").pop()?.toLowerCase() || "";
  return EXT_ICONS[ext] || "📄";
}

function TreeNode({ node, depth, selectedPath, onSelect, onDoubleClick }: {
  node: FileNode; depth: number;
  selectedPath: string | null;
  onSelect: (path: string) => void;
  onDoubleClick: (path: string) => void;
}): ReactElement {
  const [open, setOpen] = useState(depth < 2);
  const pad = depth * 14;

  if (node.isDir) {
    return (
      <div>
        <div onClick={() => setOpen(!open)}
          style={{ display: "flex", alignItems: "center", gap: 4, padding: "2px 4px", cursor: "pointer",
            fontSize: 12, color: "var(--fg-muted)", userSelect: "none", paddingLeft: pad,
            borderRadius: 4, transition: "background 100ms" }}
          onMouseEnter={e => (e.currentTarget.style.background = "var(--bg-hover)")}
          onMouseLeave={e => (e.currentTarget.style.background = "transparent")}>
          <span style={{ width: 14, textAlign: "center", fontSize: 10, flexShrink: 0 }}>{open ? "▾" : "▸"}</span>
          <span style={{ opacity: 0.8 }}>{getIcon(node.name, true)}</span>
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{node.name}</span>
        </div>
        {open && node.children?.map((c, i) =>
          <TreeNode key={i} node={c} depth={depth + 1} selectedPath={selectedPath} onSelect={onSelect} onDoubleClick={onDoubleClick} />
        )}
      </div>
    );
  }

  const selected = selectedPath === node.path;
  return (
    <div onClick={() => onSelect(node.path)} onDoubleClick={() => onDoubleClick(node.path)}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 6px", cursor: "pointer",
        fontSize: 12, color: "var(--fg-secondary)", paddingLeft: pad + 18, userSelect: "none",
        borderRadius: 4, background: selected ? "var(--bg-hover)" : "transparent",
        transition: "background 100ms" }}
      title={node.path}
      onMouseEnter={e => { if (!selected) e.currentTarget.style.background = "var(--bg-hover)"; }}
      onMouseLeave={e => { if (!selected) e.currentTarget.style.background = "transparent"; }}>
      <span style={{ opacity: 0.7, flexShrink: 0 }}>{getIcon(node.name, false)}</span>
      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{node.name}</span>
    </div>
  );
}

export function FileTree({ cwd, onSelect, onDoubleClick }: {
  cwd: string; onSelect?: (path: string) => void; onDoubleClick?: (path: string) => void;
}): ReactElement {
  const [tree, setTree] = useState<FileNode[]>([]);
  const [error, setError] = useState("");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!cwd) { setError("无项目目录"); return; }
    setError("");
    try {
      const paths: string[] = await window.__TAURI__?.core?.invoke<string[]>("list_project_files", { cwd });
      if (paths && paths.length > 0) setTree(buildTree(paths));
      else setError("目录为空");
    } catch (e: any) { setError(`加载失败: ${e}`); }
  }, [cwd]);

  useEffect(() => { load(); }, [load]);
  // Reload when graph updates (file changes)
  useEffect(() => {
    const tauri = window.__TAURI__;
    if (!tauri?.event?.listen) return;
    let unlisten: (() => void) | undefined;
    tauri.event.listen("graph-updated", () => load()).then(fn => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [load]);

  const handleSelect = (path: string) => setSelectedPath(path);
  const handleDoubleClick = async (path: string) => {
    if (onDoubleClick) { onDoubleClick(path); return; }
    // Try to open in system editor
    try {
      await window.__TAURI__?.core?.invoke("read_session_file", { path: `${cwd}/${path}` });
    } catch {
      // Fallback: select the file
      onSelect?.(path);
    }
  };

  if (error) {
    return <div style={{ padding: 16, fontSize: 12, color: "var(--fg-muted)", textAlign: "center" }}>{error}</div>;
  }
  if (tree.length === 0) {
    return <div style={{ padding: 16, fontSize: 12, color: "var(--fg-muted)", textAlign: "center" }}>无文件</div>;
  }

  return (
    <div style={{ overflowY: "auto", flex: 1, padding: "4px 0", maxHeight: "100%" }}>
      {tree.map((node, i) => (
        <TreeNode key={i} node={node} depth={0} selectedPath={selectedPath}
          onSelect={p => { setSelectedPath(p); onSelect?.(p); }}
          onDoubleClick={handleDoubleClick} />
      ))}
    </div>
  );
}
