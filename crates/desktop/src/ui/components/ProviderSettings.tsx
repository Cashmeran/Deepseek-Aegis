import type { ProviderConfig, ProviderKind } from "../types";

export const DEEPSEEK_MODELS = ["deepseek-v4-pro", "deepseek-v4-flash"];

export function ProviderSettings({
  value,
  onChange,
  config,
  onConfigChange,
  permissionMode,
  onPermissionModeChange
}: {
  value: ProviderKind;
  onChange: (value: ProviderKind) => void;
  config: ProviderConfig;
  onConfigChange: (value: ProviderConfig) => void;
  permissionMode: string;
  onPermissionModeChange: (value: string) => void;
}) {
  const modelOptions = DEEPSEEK_MODELS;
  const selectedModel = modelOptions.includes(config.model) ? config.model : "custom";

  return (
    <div className="rounded-xl border border-ink-900/10 bg-panel/80 px-3 py-3 backdrop-blur">
      <div className="text-xs font-medium text-muted">服务商</div>
      <div className="mt-2">
        <span className="rounded-lg border border-accent/60 bg-accent/10 text-ink-800 px-3 py-1 text-xs">DeepSeek</span>
      </div>
      <div className="mt-3 grid gap-2">
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          模型预设
          <select className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 focus:border-accent focus:outline-none"
            value={selectedModel}
            onChange={(e) => { if (e.target.value !== "custom") onConfigChange({ ...config, model: e.target.value }); }}>
            {modelOptions.map((m) => (<option key={m} value={m}>{m}</option>))}
            <option value="custom">自定义</option>
          </select>
        </label>
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          API Key
          <input type="password" className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 focus:border-accent focus:outline-none"
            placeholder="sk-..." value={config.apiKey}
            onChange={(e) => onConfigChange({ ...config, apiKey: e.target.value })} />
        </label>
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          模型
          <input className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 focus:border-accent focus:outline-none"
            placeholder="deepseek-v4-pro" value={config.model}
            onChange={(e) => onConfigChange({ ...config, model: e.target.value })} />
        </label>
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          Base URL (可选)
          <input className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 focus:border-accent focus:outline-none"
            placeholder="https://api.deepseek.com" value={config.baseUrl ?? ""}
            onChange={(e) => onConfigChange({ ...config, baseUrl: e.target.value })} />
        </label>
      </div>
      <div className="mt-4 border-t border-ink-900/10 pt-3">
        <div className="text-xs font-medium text-muted">权限</div>
        <div className="mt-2 grid gap-2">
          <button type="button" onClick={() => onPermissionModeChange("auto")}
            className={`rounded-lg border px-3 py-2 text-left text-xs ${permissionMode === "auto" ? "border-accent/60 bg-accent/10 text-ink-800" : "border-ink-900/10 bg-white text-muted hover:border-ink-900/20 hover:text-ink-700"}`}>
            自动编辑
          </button>
          <button type="button" onClick={() => onPermissionModeChange("ask")}
            className={`rounded-lg border px-3 py-2 text-left text-xs ${permissionMode === "ask" ? "border-accent/60 bg-accent/10 text-ink-800" : "border-ink-900/10 bg-white text-muted hover:border-ink-900/20 hover:text-ink-700"}`}>
            编辑前询问
          </button>
        </div>
      </div>
    </div>
  );
}
