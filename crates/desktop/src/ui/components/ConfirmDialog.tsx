// ConfirmDialog — confirm before destructive actions
import type { ReactElement } from "react";

export function ConfirmDialog({ msg, options, onChoice, onClose }: {
  msg: string;
  options: { label: string; kind: "danger" | "ghost" }[];
  onChoice: (idx: number) => void;
  onClose: () => void;
}): ReactElement {
  return (
    <div className="confirm-overlay" onClick={onClose}>
      <div className="confirm-box" onClick={e => e.stopPropagation()}>
        <p>{msg}</p>
        <div className="modal-actions">
          <button className="btn btn-ghost btn-sm" onClick={onClose}>取消</button>
          {options.map((opt, i) => (
            <button key={i} className="btn btn-primary btn-sm"
              style={opt.kind === "danger" ? {background:"var(--danger)"} : {}}
              onClick={() => onChoice(i)}>{opt.label}</button>
          ))}
        </div>
      </div>
    </div>
  );
}
