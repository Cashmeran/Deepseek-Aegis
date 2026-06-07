// Aegis logo — three-body hexagon motif.
// Three nodes connected in a circuit pattern, referencing the
// 三体分离 (Planner → Generator → Evaluator) architecture.

export function AegisLogo({ size = 56 }: { size?: number }) {
  const s = size;
  const cx = s / 2;
  const cy = s / 2;
  const r = s * 0.38;
  const dotR = s * 0.07;

  // Three node positions at 0°, 120°, 240° (rotated -90° so top is first)
  const angles = [-90, 30, 150];
  const nodes = angles.map((deg) => {
    const rad = (deg * Math.PI) / 180;
    return { x: cx + r * Math.cos(rad), y: cy + r * Math.sin(rad) };
  });

  return (
    <svg width={s} height={s} viewBox={`0 0 ${s} ${s}`} fill="none" xmlns="http://www.w3.org/2000/svg">
      {/* Connecting lines */}
      <line x1={nodes[0].x} y1={nodes[0].y} x2={nodes[1].x} y2={nodes[1].y}
        stroke="url(#aegis-line)" strokeWidth={s * 0.04} strokeLinecap="round" />
      <line x1={nodes[1].x} y1={nodes[1].y} x2={nodes[2].x} y2={nodes[2].y}
        stroke="url(#aegis-line)" strokeWidth={s * 0.04} strokeLinecap="round" />
      <line x1={nodes[2].x} y1={nodes[2].y} x2={nodes[0].x} y2={nodes[0].y}
        stroke="url(#aegis-line)" strokeWidth={s * 0.04} strokeLinecap="round" />

      {/* Nodes */}
      {nodes.map((n, i) => (
        <circle key={i} cx={n.x} cy={n.y} r={dotR}
          fill={i === 0 ? "var(--accent)" : "var(--accent-text)"}
          opacity={i === 0 ? 1 : 0.55} />
      ))}

      {/* Center dot */}
      <circle cx={cx} cy={cy} r={dotR * 0.7} fill="var(--accent)" opacity={0.4} />

      <defs>
        <linearGradient id="aegis-line" x1="0" y1="0" x2={s} y2={s} gradientUnits="userSpaceOnUse">
          <stop stopColor="#0ea56b" />
          <stop offset="1" stopColor="#23d18b" />
        </linearGradient>
      </defs>
    </svg>
  );
}

export function AegisWordmark({ size = 17 }: { size?: number }) {
  return (
    <span style={{
      fontSize: size,
      fontWeight: 650,
      color: "var(--fg-primary)",
      letterSpacing: "-0.02em",
      display: "inline-flex",
      alignItems: "center",
      gap: "8px",
      userSelect: "none",
    }}>
      <AegisLogo size={size + 6} />
      Aegis
    </span>
  );
}
