import type { CSSProperties } from "react";
import "./tokens.css";
import "./glass.css";
import "./motion.css";

export type OrbState = "idle" | "listening" | "thinking" | "speaking";

/** Sphère d'IRIS : halo, ondes d'écoute, corps (blobs de couleur, tourbillon, reflet). Tout le mouvement est en CSS. */
export function Orb({ state, size, level = 0 }: { state: OrbState; size: number; level?: number }) {
  const style = { width: size, height: size, "--level": level } as CSSProperties;
  return (
    <div className="orb" data-state={state} style={style} aria-hidden="true">
      <span className="glow" />
      <span className="orb-waves"><i /><i /></span>
      <span className="orb-body">
        <span className="orb-swirl"><i /><i /><i /></span>
        <span className="orb-shine" />
      </span>
    </div>
  );
}
