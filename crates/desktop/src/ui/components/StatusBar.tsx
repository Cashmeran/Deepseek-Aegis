// StatusBar — model, tokens, cost, connection status
import type { ReactElement } from "react";

function contextWindow(model: string): number {
  if (model.includes("v3") || model.includes("v4")) return 1_000_000;
  if (model.includes("r1")) return 1_000_000;
  return 1_000_000; // DeepSeek V4/V3/R1 all 1M context
}

export function StatusBar({
  inputTokens, outputTokens, cost, cachePct,
  isRunning, model, connected,
}: {
  inputTokens: number; outputTokens: number; cost: number; cachePct?: number;
  isRunning: boolean; model: string; connected: boolean;
}): ReactElement {
  const ctx = contextWindow(model);
  const totalInput = inputTokens + outputTokens;
  const ctxPct = Math.round((totalInput / ctx) * 100);
  const fmtK = (n: number) => n >= 1000 ? `${(n / 1000).toFixed(1)}K` : `${n}`;

  return (
    <div className="status-bar">
      <div className="status-bar-group">
        <span className="status-bar-label">模型</span>
        <span className="status-bar-value">{model}</span>
      </div>
      <div className="status-bar-divider" />
      <div className="status-bar-group gap-xs" style={{ flex: 0 }}>
        <span className="status-bar-label">Token</span>
        <div className="status-bar-token-bar">
          <div className="status-bar-token-fill" style={{width:`${Math.min(ctxPct, 100)}%`}} />
        </div>
        <span className="status-bar-value">
          {fmtK(totalInput)}/{fmtK(ctx)} ({ctxPct}%)
        </span>
      </div>
      <div className="status-bar-divider" />
      <div className="status-bar-group">
        <span className="status-bar-label">缓存命中</span>
        <span className={`status-bar-value ${cachePct && cachePct > 50 ? "highlight" : ""}`}>
          {cachePct ?? "--"}%
        </span>
      </div>
      <div className="status-bar-divider" />
      <div className="status-bar-group">
        <span className="status-bar-label">费用</span>
        <span className="status-bar-value">¥{cost.toFixed(4)}</span>
      </div>
      <span className="spacer" />
      <div className="status-bar-group">
        <span className={`status-bar-value ${isRunning ? "highlight" : ""}`}>
          {isRunning ? "运行中" : connected ? "就绪" : "离线"}
        </span>
      </div>
    </div>
  );
}
