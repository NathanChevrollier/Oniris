import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

/** Miroir de `oniris_core::timers::Timer`. */
export type Timer = {
  id: number; label: string; kind: "timer" | "focus" | "break";
  duration_ms: number; ends_ms: number; focus_cycle: number | null;
};

/** Minuteurs factices pour l'aperçu navigateur (hors Tauri). */
function fakeTimers(): Timer[] {
  const t = (id: number, label: string, kind: Timer["kind"], dur: number, left: number): Timer =>
    ({ id, label, kind, duration_ms: dur * 60_000, ends_ms: Date.now() + left * 60_000, focus_cycle: null });
  return [t(1, "Focus", "focus", 25, 17.5), t(2, "Thé", "timer", 4, 2.5)];
}

const fakeDone: ToolOutcome = { Done: { action_id: 0, output: {} } };

export const timersIpc = {
  list: (): Promise<Timer[]> => isTauri ? invoke("timers_list") : Promise.resolve(fakeTimers()),
  start: (text: string): Promise<ToolOutcome> => isTauri ? invoke("timers_start", { text }) : Promise.resolve(fakeDone),
  focus: (): Promise<ToolOutcome> => isTauri ? invoke("focus_start") : Promise.resolve(fakeDone),
  cancel: (id: number): Promise<ToolOutcome> => isTauri ? invoke("timers_cancel", { id }) : Promise.resolve(fakeDone),
};

/** mm:ss du temps restant, arrondi à la seconde supérieure (25:00 au départ, 00:00 à la fin). */
export function mmss(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}
