import { useState, useEffect, type ReactElement } from "react";
import { I } from "../icons";

type SettingsFields = { apiKey: string; model: string; };

const AVAILABLE_MODELS = ["deepseek-v4-pro", "deepseek-v4-flash", "deepseek-v3.2", "deepseek-r1"];

type TabId = "general" | "context" | "mcp" | "logs";

export function SettingsModal({ onClose, apiKey, model, onSave, activeCwd }: {
  onClose: () => void; apiKey: string; model: string; onSave: (f: SettingsFields) => void; activeCwd?: string;
}): ReactElement {
  const [key, setKey] = useState(apiKey);
  const [showKey, setShowKey] = useState(false);
  const [mdl, setMdl] = useState(model);
  const [tab, setTab] = useState<TabId>("general");

  const [logDir, setLogDir] = useState("加载中…");
  useEffect(() => { window.__TAURI__?.core?.invoke<string>("get_log_dir").then(setLogDir).catch(() => setLogDir("不可用")); }, []);

  const [computerUseEnabled, setComputerUseEnabled] = useState(false);
  useEffect(() => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke<boolean>("get_computer_use_enabled", { cwd: activeCwd }).then(setComputerUseEnabled).catch(() => {});
  }, [activeCwd]);

  const toggleComputerUse = (v: boolean) => {
    setComputerUseEnabled(v);
    if (activeCwd) window.__TAURI__?.core?.invoke("set_computer_use_enabled", { cwd: activeCwd, enabled: v });
  };

  const [ctxMaxTurns, setCtxMaxTurns] = useState(25);
  const [ctxVerify, setCtxVerify] = useState(true);
  const [ctxMaxTokens, setCtxMaxTokens] = useState("0");
  useEffect(() => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke<{maxTurns:number;verifyBeforeOutput:boolean;maxContextTokens:number}>("get_compaction_config", { cwd: activeCwd })
      .then(c => { if (c) { setCtxMaxTurns(c.maxTurns); setCtxVerify(c.verifyBeforeOutput); setCtxMaxTokens(String(c.maxContextTokens||0)); } }).catch(() => {});
  }, [activeCwd]);

  const [mcpContent, setMcpContent] = useState("加载中…");
  useEffect(() => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke<string>("get_mcp_config", { cwd: activeCwd }).then(setMcpContent).catch(() => setMcpContent("{}"));
  }, [activeCwd]);

  const saveCtx = () => {
    if (!activeCwd) return;
    window.__TAURI__?.core?.invoke("save_compaction_config", { cwd: activeCwd, maxTurns: ctxMaxTurns, verify: ctxVerify, maxCtx: parseInt(ctxMaxTokens)||0 });
  };
  const saveMcp = () => { if (activeCwd) window.__TAURI__?.core?.invoke("save_mcp_config", { cwd: activeCwd, content: mcpContent }); };
  const openLogDir = () => { window.__TAURI__?.core?.invoke("open_log_dir").catch(() => {}); };
  const openMcpDir = () => { if (activeCwd) window.__TAURI__?.core?.invoke("open_mcp_config_dir", { cwd: activeCwd }).catch(() => {}); };

  const tabs: { id: TabId; label: string }[] = [
    { id: "general", label: "通用" }, { id: "context", label: "上下文" }, { id: "mcp", label: "MCP" }, { id: "logs", label: "日志" }
  ];

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="settings-modal" onClick={e => e.stopPropagation()} style={{display:"flex",flexDirection:"row",maxWidth:660}}>
        <div style={{width:140,flexShrink:0,borderRight:"1px solid var(--border)",padding:"12px 8px",display:"flex",flexDirection:"column",gap:2}}>
          <div className="settings-header" style={{padding:"0 8px 12px",borderBottom:"1px solid var(--border)",marginBottom:4,fontSize:14}}>设置</div>
          {tabs.map(t => (
            <button key={t.id} onClick={() => setTab(t.id)}
              style={{padding:"8px 12px",borderRadius:"var(--radius-sm)",border:"none",background:tab===t.id?"var(--bg-hover)":"transparent",color:tab===t.id?"var(--fg-primary)":"var(--fg-muted)",fontSize:13,fontWeight:500,cursor:"pointer",textAlign:"left"}}>
              {t.label}
            </button>
          ))}
          <div style={{flex:1}} />
          <button className="btn-icon" onClick={onClose} style={{marginTop:8}}><I.x /></button>
        </div>
        <div style={{flex:1,padding:"16px 20px",maxHeight:"60vh",overflowY:"auto"}}>
          {tab === "general" && <>
            <div className="settings-section">
              <h3>DeepSeek 配置</h3>
              <label style={{fontSize:12,color:"var(--fg-secondary)"}}>API Key</label>
              <div style={{display:"flex",gap:0,marginBottom:10}}>
                <input className="input" type={showKey ? "text" : "password"} value={key} onChange={e => setKey(e.target.value)} placeholder="sk-…" style={{flex:1,borderTopRightRadius:0,borderBottomRightRadius:0}} />
                <button onClick={() => setShowKey(!showKey)} style={{padding:"0 10px",border:"1px solid var(--border)",borderLeft:"none",borderRadius:"0 var(--radius-sm) var(--radius-sm) 0",background:"var(--bg-hover)",color:"var(--fg-muted)",cursor:"pointer",fontSize:11}}>
                  {showKey ? <I.eyeOff /> : <I.eye />}
                </button>
              </div>
              <label style={{fontSize:12,color:"var(--fg-secondary)"}}>Model</label>
              <select className="input" value={mdl} onChange={e => setMdl(e.target.value)}>
                {AVAILABLE_MODELS.map(m => <option key={m} value={m}>{m}</option>)}
              </select>
            </div>
            <div className="settings-section">
              <h3>计算机控制</h3>
              <div style={{display:"flex",alignItems:"center",gap:10,marginTop:8}}>
                <label style={{fontSize:13,color:"var(--fg-primary)",fontWeight:500}}>启用 Computer Use</label>
                <button onClick={() => toggleComputerUse(!computerUseEnabled)}
                  style={{width:36,height:20,borderRadius:10,border:"none",background:computerUseEnabled?"var(--accent)":"var(--border)",cursor:"pointer",position:"relative",transition:"background 150ms"}}>
                  <span style={{position:"absolute",top:2,left:computerUseEnabled?18:2,width:16,height:16,borderRadius:"50%",background:"#fff",transition:"left 150ms"}} />
                </button>
              </div>
              <div style={{fontSize:11,color:"var(--fg-muted)",marginTop:4}}>允许 Agent 控制鼠标键盘、截屏和操作桌面应用。默认关闭。开启后需重新开始对话生效。</div>
            </div>
            <div className="settings-section">
              <h3>快捷键</h3>
              <div style={{fontSize:12,color:"var(--fg-secondary)",lineHeight:1.8}}>
                <div><kbd className="kbd">Ctrl+K</kbd> 命令面板</div>
                <div><kbd className="kbd">Ctrl+B</kbd> 切换侧边栏</div>
                <div><kbd className="kbd">Ctrl+,</kbd> 设置</div>
                <div><kbd className="kbd">Enter</kbd> 发送 · <kbd className="kbd">Shift+Enter</kbd> 换行</div>
              </div>
            </div>
            <div style={{paddingTop:12,borderTop:"1px solid var(--border)"}}>
              <button className="btn btn-primary" onClick={() => { onSave({ apiKey: key, model: mdl }); onClose(); }}>保存并关闭</button>
            </div>
          </>}
          {tab === "context" && <>
            <div className="settings-section">
              <h3>上下文压缩</h3>
              <label style={{fontSize:12,color:"var(--fg-secondary)"}}>最大轮次</label>
              <input className="input" type="number" value={ctxMaxTurns} onChange={e => setCtxMaxTurns(Number(e.target.value))} min={5} max={100} style={{marginBottom:10}} />
              <div style={{fontSize:11,color:"var(--fg-muted)",marginTop:-6,marginBottom:10}}>超过此轮次数自动触发上下文折叠。建议: 15-30</div>
              <label style={{fontSize:12,color:"var(--fg-secondary)"}}>上下文上限 (tokens, 0=自动)</label>
              <input className="input" type="number" value={ctxMaxTokens} onChange={e => setCtxMaxTokens(e.target.value)} min={0} style={{marginBottom:10}} />
              <div style={{fontSize:11,color:"var(--fg-muted)",marginTop:-6,marginBottom:10}}>0 = 使用模型最大窗口的 80%。设置后覆盖自动检测</div>
              <div style={{display:"flex",alignItems:"center",gap:10,marginTop:8}}>
                <label style={{fontSize:13,color:"var(--fg-primary)",fontWeight:500}}>输出前验证</label>
                <button onClick={() => setCtxVerify(!ctxVerify)} style={{width:36,height:20,borderRadius:10,border:"none",background:ctxVerify?"var(--accent)":"var(--border)",cursor:"pointer",position:"relative",transition:"background 150ms"}}>
                  <span style={{position:"absolute",top:2,left:ctxVerify?18:2,width:16,height:16,borderRadius:"50%",background:"#fff",transition:"left 150ms"}} />
                </button>
              </div>
              <div style={{fontSize:11,color:"var(--fg-muted)",marginTop:4}}>Agent 输出代码后自动跑 cargo check + cargo test 验证</div>
              <button className="btn btn-primary btn-sm" onClick={saveCtx} style={{marginTop:12}}>保存上下文设置</button>
            </div>
          </>}
          {tab === "mcp" && <>
            <div className="settings-section">
              <div style={{display:"flex",alignItems:"center",justifyContent:"space-between",marginBottom:8}}>
                <h3 style={{margin:0}}>MCP 服务器配置</h3>
                <span style={{fontSize:11,color:"var(--fg-muted)"}}>.mcp.json</span>
              </div>
              <textarea className="textarea" value={mcpContent} onChange={e => setMcpContent(e.target.value)}
                style={{minHeight:200,fontFamily:"'JetBrains Mono',monospace",fontSize:12,lineHeight:1.5}} />
              <div style={{display:"flex",gap:8,marginTop:8}}>
                <button className="btn btn-primary btn-sm" onClick={saveMcp}>保存</button>
                <button className="btn btn-ghost btn-sm" onClick={openMcpDir}><I.folder /> 打开目录</button>
              </div>
            </div>
          </>}
          {tab === "logs" && <>
            <div className="settings-section">
              <h3>日志</h3>
              <label style={{fontSize:12,color:"var(--fg-secondary)"}}>日志目录</label>
              <div style={{display:"flex",alignItems:"center",gap:8,marginBottom:10}}>
                <code style={{flex:1,padding:"6px 10px",background:"var(--bg-hover)",borderRadius:"var(--radius-sm)",fontSize:12,fontFamily:"'JetBrains Mono',monospace",color:"var(--fg-secondary)",wordBreak:"break-all"}}>{logDir}</code>
                <button className="btn btn-ghost btn-sm" onClick={openLogDir} style={{flexShrink:0}}><I.folder /> 打开</button>
              </div>
              <div style={{fontSize:11,color:"var(--fg-muted)"}}>日志文件保存在此目录。出问题时查看最新日志排查。</div>
            </div>
          </>}
        </div>
      </div>
    </div>
  );
}
