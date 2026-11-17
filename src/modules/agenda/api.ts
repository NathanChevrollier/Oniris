import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

/** Calendrier d'origine (plusieurs quand des doublons ont été fusionnés). */
export type EventSource = { calendar: string; account: string | null; color: string | null };

/** Miroir de `oniris_core::agenda::Event`. */
export type AgendaEvent = {
  /** Table `events` ; 0 pour un événement externe (utiliser `key`). */
  id: number;
  /** Clé unique : `l:<id>` (local), `g:<compte>:<agenda>:<id>` (Google) ou `c:<calendrier>:<n>` (.ics). */
  key: string;
  source: "local" | "google" | "ics";
  /** Lecture seule : ni déplaçable ni supprimable. */
  readonly: boolean;
  title: string; start_ms: number; end_ms: number | null; all_day: boolean; color: string | null; notes: string | null;
  /** Nom du calendrier (externes). */
  calendar?: string | null;
  sources?: EventSource[];
};

/** Miroir de `oniris_core::agenda::calendars::Calendar`. */
export type Calendar = {
  id: number; kind: "google" | "file" | "url"; account_id: number | null; account: string | null; name: string; color: string | null;
  primary: boolean; visible: boolean;
  /** Masqué par défaut : déjà affiché depuis un autre compte. */
  duplicate: boolean;
  url: string | null; fetched_ms: number | null; error: string | null; events: number;
};

/** Compte rendu d'un import ou d'un abonnement. */
export type CalendarReport = { name: string; events: number; truncated: boolean };

/** Événements factices pour l'aperçu navigateur (hors Tauri) : aujourd'hui et quelques jours du mois. */
function fakeEvents(): AgendaEvent[] {
  const at = (dayOffset: number, h: number, m = 0) => { const d = new Date(); d.setDate(d.getDate() + dayOffset); d.setHours(h, m, 0, 0); return d.getTime(); };
  const mk = (id: number, title: string, start_ms: number, end_ms: number | null = null): AgendaEvent =>
    ({ id, key: `l:${id}`, source: "local", readonly: false, title, start_ms, end_ms, all_day: false, color: null, notes: null });
  return [
    mk(1, "Point projet", at(0, 10), at(0, 11)),
    mk(2, "Dentiste", at(0, 14, 30), at(0, 15, 15)),
    { ...mk(0, "Réunion d'équipe", at(0, 16), at(0, 17)), key: "g:1:cal:fake", source: "google", readonly: true, color: "#0a84ff",
      calendar: "Travail", sources: [{ calendar: "Travail", account: "prenom.nom@gmail.com", color: "#0a84ff" }] },
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

const fakeCalendars: Calendar[] = [
  { id: 1, kind: "google", account_id: 1, account: "prenom.nom@gmail.com", name: "prenom.nom@gmail.com", color: "#0a84ff", primary: true, visible: true, duplicate: false, url: null, fetched_ms: null, error: null, events: 12 },
  { id: 2, kind: "google", account_id: 1, account: "prenom.nom@gmail.com", name: "Jours fériés en France", color: "#16a765", primary: false, visible: true, duplicate: false, url: null, fetched_ms: null, error: null, events: 3 },
  { id: 3, kind: "google", account_id: 2, account: "projets@gmail.com", name: "Jours fériés en France", color: "#16a765", primary: false, visible: false, duplicate: true, url: null, fetched_ms: null, error: null, events: 0 },
];

export const calendarsIpc = {
  list: (): Promise<Calendar[]> => isTauri ? invoke("agenda_calendars") : Promise.resolve(fakeCalendars),
  setVisible: (id: number, visible: boolean): Promise<void> => isTauri ? invoke("agenda_calendar_set_visible", { id, visible }) : Promise.resolve(),
  remove: (id: number): Promise<void> => isTauri ? invoke("agenda_calendar_delete", { id }) : Promise.resolve(),
  importPath: (path: string): Promise<CalendarReport> => invoke("agenda_import_ics", { path }),
  /** Boîte de dialogue puis import ; `null` si l'utilisateur annule. */
  importFile: async (): Promise<CalendarReport | null> => {
    if (!isTauri) return null;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ multiple: false, filters: [{ name: "Calendrier", extensions: ["ics"] }] });
    return typeof path === "string" ? calendarsIpc.importPath(path) : null;
  },
  subscribe: (url: string): Promise<CalendarReport> => isTauri ? invoke("agenda_subscribe", { url }) : Promise.reject("Indisponible dans l'aperçu."),
  /** Relève les abonnements vieux de plus de 6 h (appelé à l'ouverture du panneau). */
  refresh: (): Promise<void> => isTauri ? invoke("agenda_refresh") : Promise.resolve(),
};

/** « Club : 42 événements » (+ mention si le fichier a été tronqué). */
export const reportText = (r: CalendarReport) =>
  `${r.name} : ${r.events} événement${r.events > 1 ? "s" : ""}${r.truncated ? " (les plus lointains ont été ignorés)" : ""}`;

/** Clé de jour locale (AAAA-MM-JJ). */
export const dayKey = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

export const dayBounds = (d: Date): [number, number] => {
  const a = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  return [a.getTime(), new Date(a.getFullYear(), a.getMonth(), a.getDate() + 1).getTime()];
};
