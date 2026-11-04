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
