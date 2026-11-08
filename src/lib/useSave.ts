import { createElement, useState } from "react";
import { ipc } from "./ipc";
import type { Settings } from "./settings";
import { errorText } from "../modules/accounts/api";

/** Relit les réglages juste avant d'écrire (comme /météo) ; le texte de refus de Rust s'affiche tel quel. */
export function useSave() {
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const save = async (patch: Partial<Settings>): Promise<boolean> => {
    try {
      const fresh = await ipc.getSettings();
      await ipc.saveSettings({ ...fresh, ...patch });
      setMsg({ ok: true, text: "Enregistré." });
      return true;
    } catch (e) { setMsg({ ok: false, text: errorText(e, "Enregistrement impossible.") }); return false; }
  };
  const note = msg && createElement("p", { className: msg.ok ? "acc-ok" : "err", role: msg.ok ? "status" : "alert" }, msg.text);
  return [save, note] as const;
}
