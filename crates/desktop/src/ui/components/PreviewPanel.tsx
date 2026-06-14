// PreviewPanel — embedded browser preview with device presets
import { useState, useCallback, useEffect, type ReactElement } from "react";
import { I } from "../icons";

type DevicePreset = "desktop" | "tablet" | "mobile" | "responsive";

const PRESETS: { id: DevicePreset; label: string; width: number | null; height: number | null }[] = [
  { id: "responsive", label: "自适应", width: null, height: null },
  { id: "desktop", label: "桌面", width: 1440, height: 900 },
  { id: "tablet", label: "平板", width: 768, height: 1024 },
  { id: "mobile", label: "手机", width: 375, height: 812 },
];

function openExternal(url: string) {
  try {
    window.__TAURI__?.shell?.open(url);
  } catch {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

export function PreviewPanel({ defaultUrl = "" }: { defaultUrl?: string }): ReactElement {
  const [url, setUrl] = useState(defaultUrl);
  const [activeUrl, setActiveUrl] = useState(defaultUrl);
  const [preset, setPreset] = useState<DevicePreset>("responsive");
  const [zoom, setZoom] = useState(1);

  const current = PRESETS.find(p => p.id === preset)!;
  const frameWidth = current.width ? Math.round(current.width * zoom) : undefined;
  const frameHeight = current.height ? Math.round(current.height * zoom) : undefined;

  // Auto-load when defaultUrl changes (e.g. agent started a dev server)
  useEffect(() => {
    if (defaultUrl && defaultUrl !== activeUrl) {
      setUrl(defaultUrl);
      setActiveUrl(defaultUrl);
    }
  }, [defaultUrl]);

  const navigate = useCallback(() => {
    let u = url.trim();
    if (!u) return;
    if (!/^https?:\/\//.test(u)) u = "http://" + u;
    setActiveUrl(u);
    setUrl(u);
  }, [url]);

  return (
    <div className="preview-panel">
      <div className="preview-toolbar">
        <div className="preview-url-row">
          <input
            className="preview-url-input"
            value={url}
            onChange={e => setUrl(e.target.value)}
            onKeyDown={e => { if (e.key === "Enter") navigate(); }}
            placeholder="输入 URL 或端口号，如 localhost:5173"
            spellCheck={false}
          />
          <button className="btn btn-primary btn-sm" onClick={navigate} title="加载">加载</button>
          <button className="btn btn-ghost btn-sm" onClick={() => openExternal(activeUrl)} title="在浏览器中打开">
            <I.external /> 打开
          </button>
        </div>
        <div className="preview-controls">
          <div className="preview-presets">
            {PRESETS.map(p => (
              <button
                key={p.id}
                className={`preview-preset-btn ${preset === p.id ? "active" : ""}`}
                onClick={() => setPreset(p.id)}
              >
                {p.id === "responsive" ? <I.globe /> : p.id === "mobile" ? <I.smartphone /> : p.id === "tablet" ? <I.tablet /> : <I.monitor />}
                {p.label}
              </button>
            ))}
          </div>
          <div className="preview-zoom">
            <button className="btn-icon btn-sm" onClick={() => setZoom(z => Math.max(0.25, z - 0.25))} disabled={zoom <= 0.25}>
              <I.minus />
            </button>
            <span className="preview-zoom-label">{Math.round(zoom * 100)}%</span>
            <button className="btn-icon btn-sm" onClick={() => setZoom(z => Math.min(2, z + 0.25))} disabled={zoom >= 2}>
              <I.plus />
            </button>
          </div>
        </div>
      </div>
      <div className="preview-frame-container">
        {activeUrl ? (
          <iframe
            className="preview-iframe"
            src={activeUrl}
            style={{
              width: frameWidth ?? "100%",
              height: frameHeight ?? "100%",
              maxWidth: frameWidth ? `${frameWidth}px` : "100%",
              maxHeight: frameHeight ? `${frameHeight}px` : "100%",
            }}
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
            title="预览"
          />
        ) : (
          <div className="preview-empty">
            <div className="preview-empty-icon"><I.globe /></div>
            <div className="preview-empty-text">
              输入 dev server 地址开始预览
            </div>
            <div className="preview-empty-hint">
              Agent 可以用 LSP tool 检测运行中的端口
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
