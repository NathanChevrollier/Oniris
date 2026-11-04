import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

/** Miroir de `oniris_core::notes::Note`. */
export type Note = { id: number; body: string; pinned: boolean; created_ms: number; updated_ms: number };

/** Notes factices pour l'aperçu navigateur (hors Tauri). */
function fakeNotes(): Note[] {
  const ago = (min: number) => Date.now() - min * 60_000;
  return [
    { id: 1, body: "Idée : widget météo discret dans la pilule", pinned: true, created_ms: ago(3000), updated_ms: ago(3000) },
    { id: 2, body: "Réunion client jeudi\nPréparer la démo et les devis", pinned: false, created_ms: ago(40), updated_ms: ago(40) },
    { id: 3, body: "Penser au pain", pinned: false, created_ms: ago(5), updated_ms: ago(5) },
  ];
}

const fakeDone: ToolOutcome = { Done: { action_id: 0, output: {} } };
const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

export const notesIpc = {
  /** Requête vide : notes récentes ; sinon recherche par préfixe de chaque mot. */
  list: (query = ""): Promise<Note[]> => {
    if (isTauri) return invoke("notes_list", { query });
    const words = fold(query).split(/\s+/).filter(Boolean);
    return Promise.resolve(fakeNotes().filter((n) => words.every((w) => fold(n.body).split(/\W+/).some((t) => t.startsWith(w)))));
  },
  create: (text: string): Promise<ToolOutcome> => isTauri ? invoke("notes_create", { text }) : Promise.resolve(fakeDone),
  pin: (id: number, pinned: boolean): Promise<ToolOutcome> => isTauri ? invoke("notes_pin", { id, pinned }) : Promise.resolve(fakeDone),
  toReminder: (id: number): Promise<ToolOutcome> => isTauri ? invoke("notes_to_reminder", { id }) : Promise.resolve(fakeDone),
  remove: (id: number): Promise<ToolOutcome> => isTauri ? invoke("notes_delete", { id }) : Promise.resolve(fakeDone),
};
