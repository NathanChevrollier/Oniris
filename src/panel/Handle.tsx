import { useRef, type KeyboardEvent, type PointerEvent, type RefObject } from "react";
import { ipc } from "../lib/ipc";
import { MIN_PCT, releaseHeight, savePanelHeight } from "./surface";

const usable = () => window.innerHeight - 28; // même marge que max-height dans panel.css

/**
 * Poignée du bas : glisser change la hauteur du panneau (aperçu direct, enregistrée au relâcher), un glissé franc
 * vers le haut le ferme, double-clic ou Entrée = hauteur automatique. Flèches : ±5 %, Début/Fin : bornes.
 */
export function Handle({ pct, panel, inert }: { pct: number; panel: RefObject<HTMLElement | null>; inert?: boolean }) {
  const drag = useRef<{ y: number; h: number } | null>(null);
  const el = () => panel.current;
  /** Hauteur affichée tout de suite (0 = automatique), sans attendre le réglage. */
  const show = (p: number) => {
    if (p) { el()?.style.setProperty("--panel-pct", String(p)); el()?.setAttribute("data-sized", ""); }
    else { el()?.style.removeProperty("--panel-pct"); el()?.removeAttribute("data-sized"); }
  };
  const current = () => pct || Math.round(((el()?.getBoundingClientRect().height ?? usable()) / usable()) * 100);
  const commit = (p: number) => { show(p); if (p !== pct) void savePanelHeight(p); };

  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { y: e.clientY, h: el()?.getBoundingClientRect().height ?? 0 };
    el()?.setAttribute("data-drag", "");
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    const max = usable(), target = d.h + e.clientY - d.y, min = (max * MIN_PCT) / 100;
    show(Math.min(Math.max(target, min), max) / max * 100);
    // Sous la hauteur minimale, le panneau suit le doigt vers le haut : le geste « remonter » se voit.
    el()?.style.setProperty("--pull", `${Math.min(0, target - min)}px`);
  };
  const end = (e: PointerEvent<HTMLDivElement>, cancel: boolean) => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    el()?.removeAttribute("data-drag");
    const dy = e.clientY - d.y;
    const r = releaseHeight(d.h, dy, usable());
    if (!cancel && r.close) { void ipc.panelClose(); return; } // --pull reste : la remontée part de là (retiré à la réouverture)
    el()?.style.removeProperty("--pull");
    if (cancel || r.close || Math.abs(dy) < 3) show(pct); // simple clic ou geste annulé : rien ne change
    else commit(r.pct);
  };
  const key = (e: KeyboardEvent<HTMLDivElement>) => {
    const c = current();
    const p = { ArrowUp: c - 5, ArrowDown: c + 5, Home: MIN_PCT, End: 100, Enter: 0 }[e.key];
    if (p === undefined) return;
    e.preventDefault();
    commit(p && Math.min(Math.max(p, MIN_PCT), 100));
  };

  return (
    <div className="handle" inert={inert} role="separator" aria-orientation="horizontal" tabIndex={0} aria-label="Hauteur du panneau"
      aria-valuemin={MIN_PCT} aria-valuemax={100} aria-valuenow={pct || undefined} aria-valuetext={pct ? `${pct} %` : "automatique"}
      title="Glisser pour redimensionner, vers le haut pour fermer ; double-clic : hauteur automatique"
      onPointerDown={down} onPointerMove={move} onPointerUp={(e) => end(e, false)} onPointerCancel={(e) => end(e, true)} onDoubleClick={() => commit(0)} onKeyDown={key} />
  );
}
