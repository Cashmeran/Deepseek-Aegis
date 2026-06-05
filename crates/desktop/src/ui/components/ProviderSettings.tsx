import type { PermissionMode, 服务商Config, 服务商Kind } from "../types";

export const DEEPSEEK_MODELS = [
  "deepseek-v4-pro",
  "deepseek-v4-flash",
];

export const ANTHROPIC_MODELS = [
  "claude-sonnet-4-5-20250929",
  "claude-3-5-sonnet-20241022",
  "claude-3-5-haiku-20241022",
  "claude-3-opus-20240229"
];

export const OPENAI_MODELS = [
  "gpt-4o",
  "gpt-4o-mini",
  "gpt-4.1",
  "gpt-4.1-mini",
  "gpt-4.1-nano"
];

export function 服务商Settings({
  value,
  onChange,
  config,
  onConfigChange,
  permissionMode,
  onPermissionModeChange
}: {
  value: 服务商Kind;
  onChange: (value: 服务商Kind) => void;
  config: 服务商Config;
  onConfigChange: (value: 服务商Config) => void;
  permissionMode: PermissionMode;
  onPermissionModeChange: (value: PermissionMode) => void;
}) {
  const modelOptions = value === "deepseek" ? DEEPSEEK_MODELS : value === "anthropic" ? ANTHROPIC_MODELS : OPENAI_MODELS;
  const selected模型 = modelOptions.includes(config.model) ? config.model : "custom";
  const apiKeyPlaceholder = value === "deepseek" ? "sk-..." : value === "anthropic" ? "sk-ant-..." : "sk-...";
  const urlPlaceholder = value === "deepseek" ? "https://api.deepseek.com/v1/chat/completions" : value === "anthropic" ? "https://api.anthropic.com/v1/messages" : "https://api.openai.com/v1/chat/completions";
  return (
    <div className="rounded-xl border border-ink-900/10 bg-panel/80 px-3 py-3 backdrop-blur">
      <div className="text-xs font-medium text-muted">服务商</div>
      <div className="mt-2 flex gap-2">
        <button
          type="button"
          onClick={() => onChange("deepseek")}
          className={`rounded-lg border px-3 py-1 text-xs ${
            value === "deepseek"
              ? "border-accent/60 bg-accent/10 text-ink-800"
              : "border-ink-900/10 bg-panel text-muted hover:border-ink-900/20 hover:text-ink-700"
          }`}
        >
          DeepSeek
        </button>
        <button
          type="button"
          onClick={() => onChange("anthropic")}
          className={`rounded-lg border px-3 py-1 text-xs ${
            value === "anthropic"
              ? "border-accent/60 bg-accent/10 text-ink-800"
              : "border-ink-900/10 bg-panel text-muted hover:border-ink-900/20 hover:text-ink-700"
          }`}
        >
          Anthropic
        </button>
        <button
          type="button"
          onClick={() => onChange("openai")}
          className={`rounded-lg border px-3 py-1 text-xs ${
            value === "openai"
              ? "border-accent/60 bg-accent/10 text-ink-800"
              : "border-ink-900/10 bg-panel text-muted hover:border-ink-900/20 hover:text-ink-700"
          }`}
        >
          OpenAI
        </button>
      </div>
      <div className="mt-3 grid gap-2">
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          模型预设
          <select
            className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent/20"
            value={selected模型}
            onChange={(event) => {
              const next = event.target.value;
              if (next === "custom") return;
              onConfigChange({ ...config, model: next });
            }}
          >
            {modelOptions.map((model) => (
              <option key={model} value={model}>{model}</option>
            ))}
            <option value="custom">Custom</option>
          </select>
        </label>
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          API Key
          <input
            type="password"
            className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 placeholder:text-muted-light focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent/20"
            placeholder={apiKeyPlaceholder}
            value={config.apiKey}
            onChange={(event) => onConfigChange({ ...config, apiKey: event.target.value })}
          />
        </label>
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          模型
          <input
            className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 placeholder:text-muted-light focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent/20"
            placeholder={modelOptions[0]}
            value={config.model}
            onChange={(event) => onConfigChange({ ...config, model: event.target.value })}
          />
        </label>
        <label className="grid gap-1 text-[11px] font-medium text-muted">
          Base URL (可选)
          <input
            className="rounded-lg border border-ink-900/10 bg-white px-3 py-2 text-xs text-ink-800 placeholder:text-muted-light focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent/20"
            placeholder={urlPlaceholder}
            value={config.baseUrl ?? ""}
            onChange={(event) => onConfigChange({ ...config, baseUrl: event.target.value })}
          />
        </label>
      </div>
      <div className="mt-4 border-t border-ink-900/10 pt-3">
        <div className="text-xs font-medium text-muted">权限</div>
        <div className="mt-2 grid gap-2">
          <button
            type="button"
            onClick={() => onPermissionModeChange("auto")}
            className={`rounded-lg border px-3 py-2 text-left text-xs ${
              permissionMode === "auto"
                ? "border-accent/60 bg-accent/10 text-ink-800"
                : "border-ink-900/10 bg-white text-muted hover:border-ink-900/20 hover:text-ink-700"
            }`}
          >
            自动编辑
          </button>
          <button
            type="button"
            onClick={() => onPermissionModeChange("ask")}
            className={`rounded-lg border px-3 py-2 text-left text-xs ${
              permissionMode === "ask"
                ? "border-accent/60 bg-accent/10 text-ink-800"
                : "border-ink-900/10 bg-white text-muted hover:border-ink-900/20 hover:text-ink-700"
            }`}
          >
            编辑前询问
          </button>
        </div>
      </div>
    </div>
  );
}
