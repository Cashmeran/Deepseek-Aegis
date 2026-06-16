// Aegis logo — simple shield

export function AegisLogo({ size = 56 }: { size?: number }) {
  const s = size;
  const pad = s * 0.15;
  const w = s - pad * 2;
  const h = s - pad * 1.5;
  return (
    <svg width={s} height={s} viewBox={`0 0 ${s} ${s}`} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d={`M${s / 2},${pad} L${s - pad},${pad + h * 0.35} L${s - pad},${pad + h * 0.65} L${s / 2},${s - pad * 0.6} L${pad},${pad + h * 0.65} L${pad},${pad + h * 0.35} Z`}
        fill="var(--accent)"
        opacity={0.85}
      />
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
