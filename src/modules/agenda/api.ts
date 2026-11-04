import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

/** Miroir de `oniris_core::agenda::Event`. */
export type AgendaEvent = {
  id: number; title: string; start_ms: number; end_ms: number | null; all_day: boolean; color: string | null; notes: string | null;
};

/** Événements factices pour l'aperçu navigateur (hors Tauri) : aujourd'hui et quelques jours du mois. */
function fakeEvents(): AgendaEvent[] {
  const at = (dayOffset: number, h: number, m = 0) => { const d = new Date(); d.setDate(d.getDate() + dayOffset); d.setHours(h, m, 0, 0); return d.getTime(); };
  const mk = (id: number, title: string, start_ms: number, end_ms: number | null = null): AgendaEvent =>
    ({ id, title, start_ms, end_ms, all_day: false, color: null, notes: null });
  return [
    mk(1, "Point projet", at(0, 10), at(0, 11)),
    mk(2, "Dentiste", at(0, 14, 30), at(0, 15, 15)),
    mk(3, "Déjeuner avec Léa", at(2, 12, 30), at(2, 14)),
    mk(4, "Livraison serveur", at(5, 9)),
    mk(5, "Revue de code", at(9, 16), at(9, 17)),
    mk(6, "Anniversaire de Tom", at(14, 19)),
    mk(7, "Renouveler le domaine", at(17, 10)),
    mk(8, "Sauvegarde VPS", at(-3, 8)),
  ];
}

const fakeDone: ToolOutcome = { Done: { action_id: 0, output: {} } };

export const agendaIpc = {
  between: (from_ms: number, to_ms: number): Promise<AgendaEvent[]> =>
    isTauri ? invoke("agenda_between", { fromMs: from_ms, toMs: to_ms })
      : Promise.resolve(fakeEvents().filter((e) => e.start_ms < to_ms && (e.end_ms != null && e.end_ms > e.start_ms ? e.end_ms > from_ms : e.start_ms >= from_ms))),
  /** `month` : 1 à 12. */
  monthDays: (year: number, month: number): Promise<number[]> =>
    isTauri ? invoke("agenda_month_days", { year, month })
      : Promise.resolve([...new Set(fakeEvents().map((e) => new Date(e.start_ms)).filter((d) => d.getFullYear() === year && d.getMonth() + 1 === month).map((d) => d.getDate()))]),
  create: (text: string): Promise<ToolOutcome> => isTauri ? invoke("agenda_create", { text }) : Promise.resolve(fakeDone),
  /** Passe par la carte de confirmation du panneau. */
  remove: (id: number): Promise<ToolOutcome> => isTauri ? invoke("agenda_delete", { id }) : Promise.resolve(fakeDone),
};

/** Clé de jour locale (AAAA-MM-JJ). */
export const dayKey = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

export const dayBounds = (d: Date): [number, number] => {
  const a = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  return [a.getTime(), new Date(a.getFullYear(), a.getMonth(), a.getDate() + 1).getTime()];
};
