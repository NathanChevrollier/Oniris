import { invoke, ipc, isTauri, onSettings } from "../../lib/ipc";

/** Miroir de `oniris_core::projects::Project`. */
export type Project = {
  path: string; name: string; last_activity_ms: number; branch: string | null; dirty: number; activity_14d: number[];
  plan: { file: string; done: number; total: number; current: string | null } | null;
  last_session: { id: string; updated_ms: number; title: string } | null;
  pinned: boolean; hidden: boolean;
};

export type ResumeMode = "normal" | "orchestrator";

/** Trois projets factices (maquette) pour l'aperçu navigateur. */
function fakeProjects(): Project[] {
  const day = 86_400_000, now = Date.now();
  return [
    { path: "C:/Projets/MyEmpire", name: "MyEmpire", last_activity_ms: now - day, branch: "feat/pathfinding", dirty: 4,
      activity_14d: [0, 1, 0, 2, 3, 0, 1, 4, 2, 0, 3, 5, 2, 6], plan: { file: "PLAN.md", done: 7, total: 12, current: "l'heuristique octile du pathfinding" },
      last_session: null, pinned: false, hidden: false },
    { path: "C:/Projets/Zenytt", name: "Zenytt", last_activity_ms: now - 3 * day, branch: "main", dirty: 0,
      activity_14d: [1, 0, 0, 3, 1, 2, 0, 0, 4, 1, 0, 2, 0, 1], plan: null,
      last_session: { id: "s1", updated_ms: now - 3 * day, title: "signature des builds 1Password" }, pinned: false, hidden: false },
    { path: "C:/Projets/MacroWorld", name: "MacroWorld", last_activity_ms: now - 28 * day, branch: "main", dirty: 0,
      activity_14d: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], plan: null, last_session: null, pinned: false, hidden: false },
  ];
}

export const projectsIpc = {
  /** Dossiers de projets configurés, puis à chaque changement de réglages ; renvoie la désinscription. */
  roots: (cb: (roots: string[]) => void): (() => void) => {
    if (!isTauri) { cb(["C:/Projets"]); return () => {}; }
    ipc.getSettings().then((st) => cb(st.projects_roots), () => {});
    const un = onSettings((st) => cb(st.projects_roots));
    return () => { un.then((f) => f()); };
  },
  list: (): Promise<Project[]> => isTauri ? invoke("projects_list", {}) : Promise.resolve(fakeProjects()),
  /** Projets masqués (réglages) : le cache Rust les garde, `projects_list` les filtre sauf demande. */
  listHidden: async (): Promise<Project[]> =>
    isTauri ? (await invoke<Project[]>("projects_list", { includeHidden: true })).filter((p) => p.hidden) : [],
  pin: (path: string, pinned: boolean): Promise<void> => isTauri ? invoke("projects_pin", { path, pinned }) : Promise.resolve(),
  hide: (path: string, hidden: boolean): Promise<void> => isTauri ? invoke("projects_hide", { path, hidden }) : Promise.resolve(),
  setRoots: (roots: string[]): Promise<void> => isTauri ? invoke("projects_set_roots", { roots }) : Promise.resolve(),
  /** Message à afficher si Claude doit être lancé à la main, sinon `null` (le panneau se ferme côté Rust). */
  resume: (path: string, mode: ResumeMode): Promise<string | null> =>
    isTauri ? invoke("projects_resume", { path, mode }) : Promise.resolve(null),
  /** Boîte de dialogue de dossier ; `null` si l'utilisateur annule. */
  browse: async (): Promise<string | null> => {
    if (!isTauri) return null;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const p = await open({ directory: true, multiple: false });
    return typeof p === "string" ? p : null;
  },
  openDir: (path: string): Promise<unknown> => isTauri ? invoke("search_open", { kind: "dir", key: path }) : Promise.resolve(),
};

/** « il y a 3 j » ; `ms` ≤ 0 : jamais d'activité. */
export function ago(ms: number, now: number): string {
  if (ms <= 0) return "aucune activité";
  const m = Math.max(0, Math.floor((now - ms) / 60_000));
  if (m < 1) return "à l'instant";
  if (m < 60) return `il y a ${m} min`;
  if (m < 1440) return `il y a ${Math.floor(m / 60)} h`;
  const d = Math.floor(m / 1440);
  if (d < 14) return `il y a ${d} j`;
  if (d < 60) return `il y a ${Math.floor(d / 7)} sem.`;
  return `il y a ${Math.floor(d / 30)} mois`;
}
