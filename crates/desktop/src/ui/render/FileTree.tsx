// Collapsible file tree — shows project directory structure.
// Click to preview files, double-click to open in system editor.

import { useState, useEffect, useCallback, type ReactElement } from "react";
import { I } from "../icons";

type FileNode = {
  name: string;
  path: string;
  isDir: boolean;
  children?: FileNode[];
};

function buildTree(paths: string[]): FileNode[] {
  const root: FileNode[] = [];
  for (const p of paths) {
    const parts = p.split(/[/\\]/);
    let current = root;
    for (let i = 0; i < parts.length; i++) {
      const name = parts[i];
      const isLast = i === parts.length - 1;
      let node = current.find(n => n.name === name);
      if (!node) {
        node = { name, path: parts.slice(0, i + 1).join("/"), isDir: !isLast, children: isLast ? undefined : [] };
        current.push(node);
      }
      if (!isLast && node.children) current = node.children;
    }
  }
  return root;
}

function TreeNode({ node, depth, onSelect, onDoubleClick }: {
  node: FileNode; depth: number; onSelect: (path: string) => void; onDoubleClick: (path: string) => void;
}): ReactElement {
  const [open, setOpen] = useState(depth < 1);
  const indent = depth * 16;

  if (node.isDir) {
    return (
      <div>
        <div onClick={() => setOpen(!open)}
          style={{ display: "flex", alignItems: "center", gap: "4px", padding: "2px 0", cursor: "pointer", fontSize: "12px", color: "var(--fg-muted)", userSelect: "none", paddingLeft: indent }}>
          <span style={{ width: "14px", textAlign: "center", flexShrink: 0, fontSize: "10px" }}>{open ? "▼" : "▶"}</span>
          <span style={{ display: "inline-flex", alignItems: "center", gap: "4px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            <span style={{ opacity: 0.5 }}><I.folder /></span> {node.name}
          </span>
        </div>
        {open && node.children?.map((child, i) => (
          <TreeNode key={i} node={child} depth={depth + 1} onSelect={onSelect} onDoubleClick={onDoubleClick} />
        ))}
      </div>
    );
  }

  const ext = node.name.split(".").pop()?.toLowerCase() || "";
  const FileIcon = ext === "rs" ? I.code : ext === "ts" || ext === "tsx" || ext === "js" || ext === "jsx" ? I.code : ext === "json" || ext === "toml" ? I.settings : ext === "md" ? I.file : ext === "css" || ext === "html" ? I.layout : I.file;

  return (
    <div onClick={() => onSelect(node.path)} onDoubleClick={() => onDoubleClick(node.path)}
      style={{ display: "flex", alignItems: "center", gap: "6px", padding: "2px 4px", cursor: "pointer", fontSize: "12px", color: "var(--fg-secondary)", paddingLeft: indent + 16, userSelect: "none", borderRadius: "4px", transition: "background 120ms ease-out" }}
      title={node.path}
      onMouseEnter={e => (e.currentTarget.style.background = "var(--bg-hover)")}
      onMouseLeave={e => (e.currentTarget.style.background = "transparent")}>
      <span style={{ display: "inline-flex", alignItems: "center", opacity: 0.6 }}><FileIcon /></span>
    </div>
  );
}

export function FileTree({ cwd, onSelect, onDoubleClick }: {
  cwd: string; onSelect?: (path: string) => void; onDoubleClick?: (path: string) => void;
}): ReactElement {
  const [tree, setTree] = useState<FileNode[]>([]);

  const load = useCallback(async () => {
    try {
      const paths: string[] = await window.__TAURI__?.core?.invoke<string[]>("list_project_files", { cwd });
      if (paths) setTree(buildTree(paths));
    } catch { setTree([]); }
  }, [cwd]);

  useEffect(() => { load(); }, [load]);

  if (tree.length === 0) {
    return <div style={{ padding: "16px", fontSize: "12px", color: "var(--fg-muted)", textAlign: "center" }}>无文件</div>;
  }

  return (
    <div style={{ overflowY: "auto", maxHeight: "100%", padding: "4px 0" }}>
      {tree.map((node, i) => (
        <TreeNode key={i} node={node} depth={0} onSelect={p => onSelect?.(p)} onDoubleClick={p => onDoubleClick?.(p)} />
      ))}
    </div>
  );
}
