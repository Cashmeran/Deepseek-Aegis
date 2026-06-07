// Process-section grouped message thread — each turn rendered as
// user_prompt → (reasoning → execution → output) groups.
import { useMemo, type ReactElement } from "react";
import MDContent from "./markdown";
import { TurnGroup } from "./ProcessMessage";

export function Thread({ messages, isRunning }: {
  messages: Record<string, unknown>[];
  isRunning: boolean;
}): ReactElement {
  const turns = useMemo(() => {
    const r: Record<string, unknown>[][] = [];
    let cur: Record<string, unknown>[] = [];
    for (const m of messages) {
      if (m.type === "user_prompt") { if (cur.length > 0) { r.push(cur); cur = []; } }
      cur.push(m);
    }
    if (cur.length > 0) r.push(cur);
    return r;
  }, [messages]);

  if (messages.length === 0) return <div />;

  return (
    <div>
      {turns.map((turn, ti) => {
        const userMsg = turn.find(m => m.type === "user_prompt");
        const agentMsgs = turn.filter(m => m.type !== "user_prompt" && m.type !== "usage");
        const usageMsgs = turn.filter(m => m.type === "usage");

        return (
          <div key={ti}>
            {ti > 0 && <div className="turn-divider" />}
            {userMsg && (
              <div className="msg-row msg-user">
                <div className="msg-label">You</div>
                <div className="msg-bubble"><MDContent text={String((userMsg as Record<string,unknown>).prompt ?? "")} /></div>
              </div>
            )}
            {agentMsgs.length > 0 && <TurnGroup messages={agentMsgs} isRunning={isRunning} />}
            {usageMsgs.map((m, i) => (
              <div key={i} className="msg-usage">
                <span>{(m as Record<string,unknown>).input_tokens as number} in / {(m as Record<string,unknown>).output_tokens as number} out · ¥{Number((m as Record<string,unknown>).cost).toFixed(4)}</span>
              </div>
            ))}
          </div>
        );
      })}
      {isRunning && (<div className="running-indicator"><div className="dot-pulse" /> Agent 工作中…</div>)}
    </div>
  );
}
