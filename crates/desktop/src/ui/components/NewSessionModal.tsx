// NewSessionModal — create a new project / session
import type { ReactElement } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { I } from "../icons";

export function NewSessionModal({
  projectName, setProjectName, cwd, setCwd, prompt, setPrompt,
  onClose, onCreate, scanning, scanResult,
}: {
  projectName: string; setProjectName: (v: string) => void;
  cwd: string; setCwd: (v: string) => void;
  prompt: string; setPrompt: (v: string) => void;
  onClose: () => void; onCreate: () => void;
  scanning?: boolean;
  scanResult?: { total_files: number; languages: { name: string; files: number }[]; duration_ms: number } | null;
}): ReactElement {
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

export function AboutModal({ onClose }: { onClose: () => void }): ReactElement {
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
