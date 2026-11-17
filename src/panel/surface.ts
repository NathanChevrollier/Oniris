import { useRef, useSyncExternalStore, type MouseEvent, type PointerEvent } from "react";
import { ipc } from "../lib/ipc";
import type { Settings } from "../lib/settings";

/** Mesure de Rust (`oniris://glass`, backdrop.rs) : opacité lisible, part couverte par une application, et pourquoi. */
export type GlassMeasure = { base: number; app: number; tone: "sombre" | "chargé" | "clair" };

/**
 * Opacité du verre (0..1) : réglage manuel, ou mesure en automatique. Une application derrière renforce le verre
 * (`boost`) ; `opacity` y reste la préférence de l'utilisateur, en décalage autour de 58.
 */
export function glassAlpha(mode: Settings["glass_mode"], opacity: number, m: GlassMeasure | null, boost = true): number {
  if (mode !== "auto" || !m) return opacity / 100;
  let a = m.base;
  if (boost && m.app > 0) a = Math.max(a + 0.3 * m.app, 0.55 + 0.3 * m.app);
  return Math.min(Math.max(a + ((opacity - 58) / 100) * 0.6, 0.25), 0.96);
}

let measure: GlassMeasure | null = null;
const subs = new Set<() => void>();
export const setGlassMeasure = (m: GlassMeasure) => { measure = m; subs.forEach((f) => f()); };
/** Dernière mesure (panneau et aperçu des Réglages). */
export const useGlassMeasure = () => useSyncExternalStore((f) => { subs.add(f); return () => void subs.delete(f); }, () => measure);

/** Variable lue par le panneau et les feuilles (fond et flou) : posée sur <html>, aucun rendu React. */
export const applyGlass = (a: number) => document.documentElement.style.setProperty("--glass-a", a.toFixed(3));

export const MIN_PCT = 40;
/** Au-delà de ce dépassement sous la hauteur minimale, le glissé vers le haut ferme le panneau. */
export const CLOSE_OVERSHOOT = 80;

/** Fin du glissé de la poignée : fermeture, ou hauteur en % de la hauteur utile `maxPx`, bornée. */
export function releaseHeight(startPx: number, dy: number, maxPx: number): { close: true } | { close: false; pct: number } {
  const target = startPx + dy;
  if (target < (maxPx * MIN_PCT) / 100 - CLOSE_OVERSHOOT) return { close: true };
  return { close: false, pct: Math.min(Math.max(Math.round((target / maxPx) * 100), MIN_PCT), 100) };
}

let chain: Promise<unknown> = Promise.resolve();
/** Réglages relus juste avant l'écriture, écritures en file (flèches répétées). */
export const savePanelHeight = (pct: number) =>
  (chain = chain.then(() => ipc.getSettings().then((s) => ipc.saveSettings({ ...s, panel_height_pct: pct }))).catch(() => {}));

/**
 * Clic « dans le vide » : pressé et relâché sur un élément marqué `data-void` (pas sur un de ses enfants),
 * sans sélection de texte. Un glissé parti d'un champ ou d'un texte ne compte donc jamais.
 */
export function useVoidClick(onVoid: () => void) {
  const down = useRef<EventTarget | null>(null);
  return {
    onPointerDown: (e: PointerEvent) => { down.current = e.target; },
    onClick: (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      const ok = t === down.current && t.hasAttribute?.("data-void") && !window.getSelection()?.toString();
      down.current = null;
      if (ok) onVoid();
    },
  };
}
