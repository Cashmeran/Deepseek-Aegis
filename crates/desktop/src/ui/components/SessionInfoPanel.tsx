// SessionInfoPanel — session metadata + usage metrics
import { type ReactElement } from "react";

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function fmtCost(n: number): string {
  if (n >= 1) return `¥${n.toFixed(2)}`;
  if (n >= 0.01) return `¥${n.toFixed(4)}`;
  return `¥${n.toFixed(6)}`;
}

export function SessionInfoPanel({
  title, status, messageCount,
  connected, inputTokens, outputTokens, cacheTokens, cost, cachePct,
}: {
  title: string; status: string; messageCount: number;
  connected: boolean;
  inputTokens: number; outputTokens: number; cacheTokens: number; cost: number; cachePct?: number;
}): ReactElement {
  const totalTokens = inputTokens + outputTokens;
  const cacheRate = cachePct ?? (inputTokens > 0 ? Math.round((cacheTokens / (inputTokens + cacheTokens)) * 100) : 0);

  return (
    <div className="session-info-panel">
      {/* Session header */}
      <div className="sip-section">
        <div className="sip-header-row">
          <span className="sip-title">{title}</span>
          <span className={`sip-status-badge ${status}`}>
            {status === "running" ? "运行中" : status === "idle" ? "就绪" : "已完成"}
          </span>
        </div>
        <div className="sip-meta">{messageCount} 条消息 · {connected ? "已连接" : "离线"}</div>
      </div>

      {/* Usage stats */}
      <div className="sip-section">
        <div className="sip-section-title">用量与费用</div>
        <div className="sip-stats-grid">
          <div className="sip-stat">
            <div className="sip-stat-value">{fmtCost(cost)}</div>
            <div className="sip-stat-label">总费用</div>
          </div>
          <div className="sip-stat">
            <div className={`sip-stat-value ${cacheRate > 80 ? "good" : cacheRate < 50 ? "bad" : ""}`}>{cacheRate}%</div>
            <div className="sip-stat-label">缓存命中</div>
          </div>
          <div className="sip-stat">
            <div className="sip-stat-value">{fmtTokens(inputTokens)}</div>
            <div className="sip-stat-label">输入</div>
          </div>
          <div className="sip-stat">
            <div className="sip-stat-value">{fmtTokens(outputTokens)}</div>
            <div className="sip-stat-label">输出</div>
          </div>
        </div>
      </div>
    </div>
  );
}
