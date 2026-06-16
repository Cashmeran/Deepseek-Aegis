import { useEffect, useState, useMemo, useCallback, type ReactElement } from "react";
import {
  ReactFlow, Background, Controls, MiniMap,
  useNodesState, useEdgesState, type Node, type Edge,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import dagre from "dagre";

type GraphData = { nodes: Node[]; edges: Edge[] };

const LANG_COLORS: Record<string, string> = {
  rust: "#f74c00", typescript: "#3178c6", tsx: "#61dafb", javascript: "#f7df1e",
  js: "#f7df1e", python: "#3572A5", go: "#00ADD8", ts: "#3178c6",
};

function layoutGraph(nodes: Node[], edges: Edge[]): Node[] {
  const g = new dagre.graphlib.Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: "LR", nodesep: 60, ranksep: 180, marginx: 40, marginy: 40 });

  for (const n of nodes) {
    g.setNode(n.id, { width: 180, height: 50 });
  }
  for (const e of edges) {
    g.setEdge(e.source, e.target);
  }

  dagre.layout(g);

  return nodes.map(n => {
    const pos = g.node(n.id);
    if (!pos) return n;
    return { ...n, position: { x: pos.x - 90, y: pos.y - 25 } };
  });
}

export function GraphPanel({ cwd }: { cwd?: string }): ReactElement {
  const [data, setData] = useState<GraphData | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<Node | null>(null);

  const load = useCallback(async () => {
    if (!cwd) { setData(null); return; }
    setLoading(true); setError("");
    try {
      const result = await window.__TAURI__?.core?.invoke<GraphData>("get_code_graph", { cwd });
      if (result) setData(result);
    } catch (e: any) {
      console.error("[GraphPanel] load failed:", e);
      setError(String(e));
    }
    setLoading(false);
  }, [cwd]);

  useEffect(() => {
    load();
    // Listen for graph-updated events from backend
    const tauri = window.__TAURI__;
    if (!tauri?.event?.listen) return;
    let unlisten: (() => void) | undefined;
    tauri.event.listen<string>("graph-updated", (event) => {
      if (event.payload === cwd || !cwd) load();
    }).then(fn => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [load, cwd]);

  const layouted = useMemo(() => {
    if (!data?.nodes?.length) return data;
    return { ...data, nodes: layoutGraph(data.nodes, data.edges) };
  }, [data]);

  const [nodes, setNodes, onNodesChange] = useNodesState(layouted?.nodes || []);
  const [edges, setEdges, onEdgesChange] = useEdgesState(layouted?.edges || []);

  useEffect(() => {
    if (layouted) { setNodes(layouted.nodes); setEdges(layouted.edges); }
  }, [layouted, setNodes, setEdges]);

  if (!cwd) {
    return <div className="graph-panel"><div className="graph-status">无项目目录</div></div>;
  }
  if (loading) {
    return <div className="graph-panel"><div className="graph-status">加载图谱…</div></div>;
  }
  if (error) {
    return <div className="graph-panel"><div className="graph-status err" style={{ wordBreak: "break-all", padding: "0 16px" }}>{error}</div></div>;
  }
  if (!data || data.nodes.length === 0) {
    return (
      <div className="graph-panel">
        <div className="graph-status">暂无图谱</div>
      </div>
    );
  }

  return (
    <div className="graph-panel">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodeClick={(_, node) => setSelected(node)}
        fitView
        fitViewOptions={{ padding: 0.3 }}
        minZoom={0.2}
        maxZoom={2}
        nodesDraggable={false}
        proOptions={{ hideAttribution: true }}
      >
        <Background color="var(--border)" gap={20} />
        <Controls showInteractive={false} />
        <MiniMap
          nodeColor={n => LANG_COLORS[(n.data as any)?.language] || "var(--fg-muted)"}
          maskColor="var(--bg-canvas)"
        />
      </ReactFlow>

      {selected && (
        <div className="graph-node-info">
          <div className="graph-node-info-title">{(selected.data as any).label}</div>
          <div className="text-xs text-muted">{(selected.data as any).path}</div>
          <div className="text-xs text-muted" style={{ marginTop: 4 }}>
            {(selected.data as any).nodeCount > 0 ? `${(selected.data as any).nodeCount} 节点` : ""}
            {(selected.data as any).language ? ` · ${(selected.data as any).language}` : ""}
          </div>
        </div>
      )}
    </div>
  );
}
