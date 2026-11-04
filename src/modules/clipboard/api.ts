import { invoke, isTauri } from "../../lib/ipc";

/** Miroir de `ClipItem` (src-tauri/src/modules/clipboard.rs) : `thumb` est un PNG en base64. */
export type ClipItem = {
  id: number;
  kind: "text" | "image";
  text: string | null;
  truncated: boolean;
  thumb: string | null;
  pinned: boolean;
  created_ms: number;
};

/** Damier 8×8 orange et gris, pour l'aperçu navigateur. */
const FAKE_THUMB = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAGUlEQVR4nGP4X+KRkJCBSTJgFQWSDINSBwAuRFxhZEEJgQAAAABJRU5ErkJggg==";

/** Entrées factices pour l'aperçu navigateur (hors Tauri). */
function fakeItems(): ClipItem[] {
  const ago = (min: number) => Date.now() - min * 60_000;
  return [
    { id: 1, kind: "text", text: "12 rue des Lilas, 75011 Paris", truncated: false, thumb: null, pinned: true, created_ms: ago(2000) },
    { id: 4, kind: "text", text: "https://open-meteo.com/en/docs", truncated: false, thumb: null, pinned: false, created_ms: ago(0) },
    { id: 3, kind: "text", text: "const total = items.reduce((s, i) => s + i.prix * i.quantite, 0);\nconsole.log(total);", truncated: false, thumb: null, pinned: false, created_ms: ago(3) },
    { id: 2, kind: "image", text: null, truncated: false, thumb: FAKE_THUMB, pinned: false, created_ms: ago(12) },
  ];
}

export const clipboardIpc = {
  /** Requête vide : entrées récentes ; sinon textes contenant la requête. */
  list: (query = ""): Promise<ClipItem[]> => {
    if (isTauri) return invoke("clipboard_list", { query });
    const q = query.trim().toLowerCase();
    return Promise.resolve(fakeItems().filter((i) => !q || (i.text ?? "").toLowerCase().includes(q)));
  },
  /** Remet l'entrée dans le presse-papiers, ferme le panneau et colle dans la fenêtre au premier plan. */
  paste: (id: number): Promise<void> => isTauri ? invoke("clipboard_paste", { id }) : Promise.resolve(),
  pin: (id: number, pinned: boolean): Promise<void> => isTauri ? invoke("clipboard_pin", { id, pinned }) : Promise.resolve(),
  remove: (id: number): Promise<void> => isTauri ? invoke("clipboard_delete", { id }) : Promise.resolve(),
};
