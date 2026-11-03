import type { CSSProperties } from "react";
import "./tokens.css";
import "./glass.css";
import "./motion.css";

export type OrbState = "idle" | "listening" | "thinking" | "speaking";

export function Orb({ state, size, level = 0 }: { state: OrbState; size: number; level?: number }) {
  const style = { width: size, height: size, "--level": level } as CSSProperties;
  return (
    <div className="orb" data-state={state} style={style}>
      <span className="glow" />
    </div>
  );
}
