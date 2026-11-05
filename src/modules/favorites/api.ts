import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

/** Miroir de `oniris_core::favorites::Favorite`. */
export type Favorite = { id: number; title: string; url: string; category: string | null; position: number; icon: string | null };

/** Favoris factices pour l'aperçu navigateur (hors Tauri). */
function fakeFavorites(): Favorite[] {
  const f = (id: number, title: string, url: string, category: string | null = null): Favorite => ({ id, title, url, category, position: id, icon: null });
  return [
    f(1, "Nexus Dashboard", "https://nexus.example.com"), f(2, "GitHub", "https://github.com"), f(3, "Zenytt", "https://zenytt.example.com"),
    f(4, "Documentation Tauri", "https://tauri.app"), f(5, "Mon blog", "https://blog.example.org"),
  ];
}

const fakeDone: ToolOutcome = { Done: { action_id: 0, output: {} } };

export const favoritesIpc = {
  list: (): Promise<Favorite[]> => isTauri ? invoke("favorites_list") : Promise.resolve(fakeFavorites()),
  reorder: (ids: number[]): Promise<void> => isTauri ? invoke("favorites_reorder", { ids }) : Promise.resolve(),
  rename: (id: number, title: string): Promise<void> => isTauri ? invoke("favorites_rename", { id, title }) : Promise.resolve(),
  add: (url: string, title?: string): Promise<ToolOutcome> => isTauri ? invoke("favorites_add", { url, title: title || null }) : Promise.resolve(fakeDone),
  remove: (id: number): Promise<ToolOutcome> => isTauri ? invoke("favorites_delete", { id }) : Promise.resolve(fakeDone),
  /** Le panneau se ferme côté Rust une fois le favori ouvert. */
  open: (id: number): Promise<ToolOutcome> => isTauri ? invoke("favorites_open", { id }) : Promise.resolve(fakeDone),
};

/** Miroir de `oniris_core::favorites::import::ImportReport`. */
export type ImportReport = { added: number; skipped_dup: number; skipped_unsafe: number };

export const importIpc = {
  /** Un rapport par navigateur trouvé. */
  browsers: (): Promise<[string, ImportReport | null][]> => isTauri ? invoke("favorites_import_browsers") : Promise.resolve([]),
  /** Boîte de dialogue puis import ; `null` si l'utilisateur annule. */
  nexusFile: async (): Promise<ImportReport | null> => {
    if (!isTauri) return null;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ multiple: false, filters: [{ name: "Nexus", extensions: ["json"] }] });
    return typeof path === "string" ? invoke<ImportReport>("favorites_import_file", { path }) : null;
  },
};

/** « 23 ajoutés · 4 déjà présents · 1 ignoré (lien non sûr) » */
export function formatReport(r: ImportReport): string {
  const parts = [`${r.added} ajouté${r.added > 1 ? "s" : ""}`];
  if (r.skipped_dup) parts.push(`${r.skipped_dup} déjà présent${r.skipped_dup > 1 ? "s" : ""}`);
  if (r.skipped_unsafe) parts.push(`${r.skipped_unsafe} ignoré${r.skipped_unsafe > 1 ? "s" : ""} (lien non sûr)`);
  return parts.join(" · ");
}
