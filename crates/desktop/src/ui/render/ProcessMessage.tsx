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
      <div style={{ marginTop: "8px" }}>
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
      <div style={{ marginTop: "4px" }}>
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          style={{
            display: "flex", alignItems: "center", gap: "6px",
            background: "none", border: "none", cursor: "pointer",
            color: "var(--fg-muted)", fontSize: "13px", fontWeight: 500,
            padding: "2px 0",
          }}
        >
          <span style={{ opacity: 0.5 }}>{expanded ? "▼" : "▶"}</span>
          <span className={isActive ? "running-indicator" : ""}>
            {isActive ? "正在思考…" : "推理过程"}
          </span>
        </button>
        {expanded && (
          <div style={{
            marginLeft: "12px", paddingLeft: "12px",
            borderLeft: "2px solid var(--border)",
            fontSize: "13px", lineHeight: "1.6",
            color: "var(--fg-secondary)",
            whiteSpace: "pre-wrap", wordBreak: "break-word",
            maxHeight: "360px", overflowY: "auto",
          }}>
            {text}
          </div>
        )}
      </div>
    );
  }

  // Execution section
  return (
    <div style={{ marginTop: "4px" }}>
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
        <div style={{
          marginLeft: "12px", paddingLeft: "12px",
          borderLeft: "2px solid var(--border)",
        }}>
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
  const isError = status === "error";
  const isPending = status === "pending" || status === "running";
  const output = (msg.output as string) || "";
  const isDiff = toolIsDiff(msg);

  const statusColor = isError ? "var(--danger)" : status === "success" ? "var(--success)" : "var(--warning)";
  const statusBg = isError ? "var(--danger-soft)" : status === "success" ? "var(--success-soft)" : "var(--warning-soft)";

  return (
    <div style={{ marginTop: "4px" }}>
      <div
        role="button"
        tabIndex={0}
        onClick={() => setOpen(!open)}
        onKeyDown={(e) => { if (e.key === "Enter") setOpen(!open); }}
        style={{
          display: "flex", alignItems: "center", gap: "8px",
          padding: "3px 8px", borderRadius: "4px",
          cursor: "pointer", fontSize: "12px",
          color: statusColor, background: statusBg,
          fontFamily: "'JetBrains Mono', monospace",
          transition: "background 120ms ease-out",
        }}
      >
        <span>{toolSummary(msg)}</span>
        {output ? (
          <span style={{ opacity: 0.5, fontSize: "10px" }}>{open ? "▲" : "▼"}</span>
        ) : null}
      </div>
      {open && output && (
        <div style={{ marginTop: "4px", marginLeft: "8px" }}>
          {isDiff ? (
            <DiffView patch={output} maxHeight={280} />
          ) : (
            <pre style={{
              padding: "8px 12px", margin: 0,
              background: "var(--bg-surface)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius-sm)",
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: "12px", lineHeight: "1.5",
              color: "var(--fg-primary)",
              whiteSpace: "pre-wrap", wordBreak: "break-all",
              maxHeight: "280px", overflowY: "auto",
            }}>
              {output}
            </pre>
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
    <div style={{ marginBottom: "16px" }}>
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
