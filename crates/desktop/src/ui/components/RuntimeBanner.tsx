// RuntimeBanner — connection error / warning banner with slide-in animation
import type { ReactElement } from "react";
import { I } from "../icons";

export function RuntimeBanner({
  error, onDismiss, onRetry,
}: {
  error: string | null;
  onDismiss: () => void;
  onRetry?: () => void;
}): ReactElement | null {
  if (!error) return null;

  return (
    <div className="runtime-banner" role="alert">
      <I.x />
      <span className="runtime-banner-text">{error}</span>
      {onRetry && (
        <button className="pill-btn" onClick={onRetry} style={{fontSize:11}}>
          重试
        </button>
      )}
      <button className="btn-icon btn-sm" onClick={onDismiss} title="关闭">
        <I.x />
      </button>
    </div>
  );
}
