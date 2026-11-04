const R = 26;
const L = 2 * Math.PI * R;

/** Anneau de la maquette ; `value` en % (0..100), `null` : pas encore de mesure. */
export function Gauge({ label, value, color }: { label: string; value: number | null; color: string }) {
  const pct = Math.min(100, Math.max(0, value ?? 0));
  return (
    <div role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={value == null ? undefined : Math.round(pct)}>
      <div className="gauge">
        <svg width="62" height="62" aria-hidden="true">
          <circle cx="31" cy="31" r={R} stroke="rgba(255,255,255,.08)" strokeWidth="6" fill="none" />
          <circle className="fill" cx="31" cy="31" r={R} stroke={color} strokeWidth="6" fill="none" strokeLinecap="round"
            strokeDasharray={L} style={{ strokeDashoffset: L * (1 - pct / 100) }} />
        </svg>
        <b>{value == null ? "–" : `${Math.round(pct)}%`}</b>
      </div>
      <small>{label}</small>
    </div>
  );
}
