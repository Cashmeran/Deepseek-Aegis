// Process-section message renderer — groups turn messages into
// reasoning → execution → output sections. Adapted from deepseek-gui.

import { useState, type ReactElement } from "react";
import MDContent from "./markdown";
import { DiffView } from "./DiffView";

type Msg = Record<string, unknown>;

// ── Grouping ──────────────────────────────────────────────────

type SectionKind = "reasoning" | "execution" | "output";

interface Section {
  kind: SectionKind;
  messages: Msg[];
}

function looksLikeUnifiedDiff(text: string): boolean {
  const lines = text.split("\n");
  return lines.some((l) => /^[+-]/.test(l) || l.startsWith("@@"));
}

export function groupSections(messages: Msg[]): Section[] {
  const sections: Section[] = [];
  for (const m of messages) {
    const t = m.type as string;
    const kind: SectionKind =
      t === "thinking" ? "reasoning" : t === "assistant" ? "output" : "execution";
    const last = sections[sections.length - 1];
    if (last && last.kind === kind) {
      last.messages.push(m);
    } else {
      sections.push({ kind, messages: [m] });
    }
  }
  return sections;
}

// ── Tool summary ──────────────────────────────────────────────

function toolSummary(m: Msg): string {
  const name = (m.name as string) || "";
  const output = (m.output as string) || "";
  const status = (m.status as string) || "pending";
  const elapsed = m.elapsed_ms ? ` ${m.elapsed_ms}ms` : "";
  const icon = status === "error" ? "✗" : status === "success" ? "✓" : "…";
  // Extract key info from output
  let detail = "";
  if (output.length > 0 && output.length < 120) {
    detail = ` · ${output.replace(/\n/g, " ").trim()}`;
  }
  return `${icon} ${name}${elapsed}${detail}`;
}

function toolIsDiff(m: Msg): boolean {
  return (m.name as string) === "file_edit" && looksLikeUnifiedDiff((m.output as string) || "");
}

// ── Section component ─────────────────────────────────────────

function ProcessSectionRow({
  section,
  isRunning,
}: {
  section: Section;
  isRunning: boolean;
}): ReactElement {
  const [expanded, setExpanded] = useState(section.kind === "execution");
  const hasDetail = section.kind === "execution" && section.messages.length > 0;
  const isActive = isRunning && section.messages.some((m) => m.status === "pending" || m.status === "running");

  if (section.kind === "output") {
    return (
      <div className="process-section">
        {section.messages.map((m, i) => (
          <div key={i} className="msg-row msg-assistant">
            <div className="msg-bubble">
              <MDContent text={String(m.text ?? "")} />
            </div>
          </div>
        ))}
      </div>
    );
  }

  if (section.kind === "reasoning") {
    const text = section.messages.map((m) => String(m.text ?? "")).join("\n\n");
    if (!text.trim()) return <></>;
    return (
      <div className="process-section">
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="process-section summary"
          style={{ display: "flex", alignItems: "center", gap: "6px", background: "none", border: "none", cursor: "pointer", color: "var(--fg-muted)", fontSize: "13px", fontWeight: 500, padding: "2px 0" }}
        >
          <span style={{ opacity: 0.5 }}>{expanded ? "▼" : "▶"}</span>
          <span className={isActive ? "running-indicator" : ""}>
            {isActive ? "正在思考…" : "推理过程"}
          </span>
        </button>
        {expanded && (
          <div className="process-section-content">
            {text}
          </div>
        )}
      </div>
    );
  }

  // Execution section
  return (
    <div className="process-section">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        style={{
          display: "flex", alignItems: "center", gap: "6px",
          background: "none", border: "none", cursor: "pointer",
          color: isActive ? "var(--accent-text)" : "var(--fg-muted)",
          fontSize: "13px", fontWeight: 500,
          padding: "2px 0",
        }}
      >
        <span style={{ opacity: 0.5 }}>{expanded ? "▼" : "▶"}</span>
        <span>{section.messages.length} 个工具调用</span>
      </button>
      {expanded && (
        <div className="process-section-content">
          {section.messages.map((m, i) => (
            <ToolCallCard key={i} msg={m} isRunning={isRunning} />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Tool call card ────────────────────────────────────────────

function ToolCallCard({ msg, isRunning }: { msg: Msg; isRunning: boolean }): ReactElement {
  const [open, setOpen] = useState(false);
  const status = (msg.status as string) || "pending";
  const output = (msg.output as string) || "";
  const isDiff = toolIsDiff(msg);

  return (
    <div className="tool-card">
      <div
        role="button"
        tabIndex={0}
        onClick={() => setOpen(!open)}
        onKeyDown={(e) => { if (e.key === "Enter") setOpen(!open); }}
        className={`tool-card-badge ${status}`}
      >
        <span>{toolSummary(msg)}</span>
        {output ? (
          <span style={{ opacity: 0.5, fontSize: "10px" }}>{open ? "▲" : "▼"}</span>
        ) : null}
      </div>
      {open && output && (
        <div className="tool-card-output">
          {isDiff ? (
            <DiffView patch={output} maxHeight={280} />
          ) : (
            <pre>{output}</pre>
          )}
        </div>
      )}
    </div>
  );
}

// ── Turn group ────────────────────────────────────────────────

export function TurnGroup({
  messages,
  isRunning,
}: {
  messages: Msg[];
  isRunning: boolean;
}): ReactElement {
  const sections = groupSections(messages);

  return (
    <div className="turn-group">
      {sections.map((section) => (
        <ProcessSectionRow
          key={`${section.kind}-${section.messages[0]?.id || Math.random()}`}
          section={section}
          isRunning={isRunning}
        />
      ))}
    </div>
  );
}
