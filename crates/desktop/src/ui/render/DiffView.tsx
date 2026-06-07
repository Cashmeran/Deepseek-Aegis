// Lightweight unified-diff renderer.
import { useMemo, useState, type ReactElement } from "react";
import { I } from "../icons";

type Props = {
  patch: string;
  className?: string;
  maxHeight?: number;
  filePath?: string;
};

type ParsedDiff = {
  filePath: string | null;
  added: number;
  removed: number;
  hunkOffset: number;
};

const LANG_BADGES: Array<{ test: RegExp; label: string }> = [
  { test: /\.rs$/i, label: "RS" },
  { test: /\.tsx?$/i, label: "TS" },
  { test: /\.jsx?$/i, label: "JS" },
  { test: /\.json$/i, label: "JSON" },
  { test: /\.(css|scss|less)$/i, label: "CSS" },
  { test: /\.md$/i, label: "MD" },
  { test: /\.py$/i, label: "PY" },
  { test: /\.html?$/i, label: "HTML" },
  { test: /\.ya?ml$/i, label: "YML" },
  { test: /\.sh$/i, label: "SH" },
  { test: /\.go$/i, label: "GO" },
  { test: /\.java$/i, label: "JAVA" },
  { test: /\.(cpp|cc|cxx|h|hpp)$/i, label: "C++" },
  { test: /\.sql$/i, label: "SQL" },
];

function parseDiff(patch: string, override?: string): ParsedDiff {
  const lines = patch.split("\n");
  let filePath = override ?? null;
  let added = 0;
  let removed = 0;
  let hunkOffset = -1;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (!filePath) {
      if (line.startsWith("+++ ")) {
        const raw = line.slice(4).trim();
        const cleaned = raw.replace(/^[ab]\//, "");
        if (cleaned && cleaned !== "/dev/null") filePath = cleaned;
      } else if (line.startsWith("--- ") && !filePath) {
        const raw = line.slice(4).trim();
        const cleaned = raw.replace(/^[ab]\//, "");
        if (cleaned && cleaned !== "/dev/null") filePath = cleaned;
      } else if (line.startsWith("diff --git ")) {
        const m = line.match(/ b\/(\S+)/);
        if (m) filePath = m[1];
      }
    }
    if (line.startsWith("@@") && hunkOffset === -1) hunkOffset = i;
    if (line.startsWith("+") && !line.startsWith("+++")) added += 1;
    else if (line.startsWith("-") && !line.startsWith("---")) removed += 1;
  }
  return { filePath, added, removed, hunkOffset };
}

function badgeFor(name: string | null): string {
  if (!name) return "TXT";
  for (const b of LANG_BADGES) if (b.test.test(name)) return b.label;
  return "TXT";
}

export function DiffView({ patch, className = "", maxHeight = 320, filePath }: Props): ReactElement {
  const lines = patch.split("\n");
  const looksLikePatch = lines.some((l) => /^[+-]/.test(l) || l.startsWith("@@"));
  const parsed = useMemo(() => parseDiff(patch, filePath), [patch, filePath]);
  const [copied, setCopied] = useState(false);

  const fileLabel = parsed.filePath ?? filePath ?? null;
  const displayName = fileLabel ? fileLabel.split(/[/\\]/).pop() ?? fileLabel : null;
  const badge = badgeFor(fileLabel);

  const onCopy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(patch);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch { /* clipboard unavailable */ }
  };

  const diffHeader = (
    <div style={{ display: "flex", alignItems: "center", gap: "8px", padding: "6px 10px", borderBottom: "1px solid var(--border)", fontSize: "12px" }}>
      <span style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: "10px", fontWeight: 600, padding: "1px 5px", borderRadius: "4px", background: "var(--bg-hover)", color: "var(--fg-secondary)" }}>
        {badge}
      </span>
      <span style={{ flex: 1, fontFamily: "'JetBrains Mono', monospace", fontSize: "12px", fontWeight: 500, color: "var(--fg-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={displayName ?? ""}>
        {displayName ?? "patch"}
      </span>
      {(parsed.added > 0 || parsed.removed > 0) && (
        <span style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: "11px", flexShrink: 0 }}>
          {parsed.added > 0 && <span style={{ color: "var(--success)" }}>+{parsed.added}</span>}
          {parsed.added > 0 && parsed.removed > 0 && <span style={{ padding: "0 4px", color: "var(--fg-muted)" }}>·</span>}
          {parsed.removed > 0 && <span style={{ color: "var(--danger)" }}>-{parsed.removed}</span>}
        </span>
      )}
      <button type="button" onClick={onCopy} style={{ background: "none", border: "none", cursor: "pointer", color: "var(--fg-muted)", padding: "2px 4px", borderRadius: "4px", fontSize: "11px" }} title="Copy diff">
        {copied ? <I.check /> : <I.copy />}
      </button>
    </div>
  );

  if (!looksLikePatch) {
    return (
      <div className={className} style={{ border: "1px solid var(--border)", borderRadius: "var(--radius)", overflow: "hidden", background: "var(--bg-surface)" }}>
        {diffHeader}
        <pre style={{ padding: "8px 12px", margin: 0, fontFamily: "'JetBrains Mono', monospace", fontSize: "11.5px", lineHeight: "1.55", color: "var(--fg-primary)", whiteSpace: "pre-wrap", overflow: "auto", maxHeight }}>
          {patch}
        </pre>
      </div>
    );
  }

  // Hide diff metadata lines in rendered body
  const bodyLines = lines
    .map((line, i) => ({ line, i }))
    .filter(({ line }) => {
      if (line.startsWith("--- ") || line.startsWith("+++ ")) return false;
      if (line.startsWith("diff --git ")) return false;
      if (line.startsWith("index ")) return false;
      return true;
    });

  // Compute display line numbers using hunk headers
  const numbered: Array<{ key: number; line: string; lineNo: number | null; bg: string; fg: string }> = [];
  let newLineNo: number | null = null;
  for (const { line, i } of bodyLines) {
    let bg = "transparent";
    let fg = "var(--fg-primary)";
    let displayedNo: number | null = null;
    if (line.startsWith("@@")) {
      bg = "var(--accent-soft)";
      fg = "var(--fg-muted)";
      const m = line.match(/\+(\d+)/);
      newLineNo = m ? parseInt(m[1], 10) : null;
    } else if (line.startsWith("+")) {
      bg = "var(--success-soft)";
      fg = "var(--success)";
      displayedNo = newLineNo;
      if (newLineNo != null) newLineNo += 1;
    } else if (line.startsWith("-")) {
      bg = "var(--danger-soft)";
      fg = "var(--danger)";
    } else {
      displayedNo = newLineNo;
      if (newLineNo != null) newLineNo += 1;
    }
    numbered.push({ key: i, line, lineNo: displayedNo, bg, fg });
  }

  return (
    <div className={className} style={{ border: "1px solid var(--border)", borderRadius: "var(--radius)", overflow: "hidden", background: "var(--bg-surface)" }}>
      {diffHeader}
      <div style={{ overflow: "auto", maxHeight, fontFamily: "'JetBrains Mono', monospace", fontSize: "11.5px", lineHeight: "1.55" }}>
        <table style={{ width: "max-content", minWidth: "100%", borderCollapse: "collapse" }}>
          <tbody>
            {numbered.map(({ key, line, lineNo, bg, fg }) => (
              <tr key={key} style={{ background: bg, color: fg }}>
                <td style={{ width: "2.75rem", textAlign: "right", padding: "0 8px", userSelect: "none", color: "var(--fg-muted)", fontVariantNumeric: "tabular-nums" }}>
                  {lineNo ?? ""}
                </td>
                <td style={{ whiteSpace: "pre", padding: "0 8px" }}>{line || " "}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
