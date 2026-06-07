// Toast — notification container
import type { ReactElement } from "react";

export type ToastItem = { id: number; text: string; kind: "success" | "error" | "info" };
let toastId = 0;

export function nextToastId(): number {
  return ++toastId;
}

export function ToastContainer({ toasts, onRemove }: { toasts: ToastItem[]; onRemove: (id: number) => void }): ReactElement | null {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-container" role="status" aria-live="polite">
      {toasts.map(t => (<div key={t.id} className={`toast ${t.kind}`} onClick={() => onRemove(t.id)}>{t.text}</div>))}
    </div>
  );
}
