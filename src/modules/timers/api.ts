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

/** Miroirs de `oniris_core::alarms::Alarm` et `modules::alarms::{Rung, AlarmsState}`. */
export type Alarm = { id: number; label: string; hour: number; minute: number; days: number; next_ms: number; repeat: string };
export type Rung = { id: number; label: string; due_ms: number };
export type AlarmsState = { alarms: Alarm[]; ringing: Rung[] };

const fakeAlarms = (): AlarmsState => ({
  alarms: [{ id: 1, label: "Réveil", hour: 7, minute: 0, days: 0x1f, next_ms: Date.now() + 9 * 3_600_000, repeat: "en semaine" }],
  ringing: [],
});

export const alarmsIpc = {
  state: (): Promise<AlarmsState> => isTauri ? invoke("alarms_state") : Promise.resolve(fakeAlarms()),
  set: (text: string): Promise<ToolOutcome> => isTauri ? invoke("alarms_set", { text }) : Promise.resolve(fakeDone),
  cancel: (id: number): Promise<ToolOutcome> => isTauri ? invoke("alarms_cancel", { id }) : Promise.resolve(fakeDone),
  stop: (): Promise<void> => invoke("alarms_stop"),
  snooze: (): Promise<void> => invoke("alarms_snooze"),
};

/** Le texte vise une alarme (« alarme 7h », « réveille-moi à 7 h », « à 14h30 », « demain 8h ») plutôt qu'un minuteur. */
export const isAlarmText = (t: string): boolean =>
  /^(alarme|r[ée]veil)/i.test(t.trim()) || /(^|\s)à\s*\d/i.test(t) || /\b(demain|minuit)\b/i.test(t);

/** « 07:00 · en semaine », « 07:00 · demain », « 07:00 · mer. 18 ». */
export function alarmWhen(a: Alarm, now = new Date()): string {
  const hm = `${String(a.hour).padStart(2, "0")}:${String(a.minute).padStart(2, "0")}`;
  if (a.days) return `${hm} · ${a.repeat}`;
  const d = new Date(a.next_ms);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(d) - day(now)) / 86_400_000);
  const when = diff === 0 ? "aujourd'hui" : diff === 1 ? "demain" : d.toLocaleDateString("fr-FR", { weekday: "short", day: "numeric" });
  return `${hm} · ${when}`;
}
