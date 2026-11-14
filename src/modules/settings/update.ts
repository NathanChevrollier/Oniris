import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke, listen } from "../../lib/ipc";

/** État de la mise à jour (champ `state`), envoyé par Rust à chaque changement. */
export type UpdatePhase =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "downloading"; version: string; percent: number | null }
  | { state: "ready"; version: string; notes: string }
  | { state: "waiting"; version: string; reason: "game" | "voice" }
  | { state: "installing"; version: string }
  | { state: "error"; message: string; version: string | null };

export type UpdateStatus = UpdatePhase & {
  current: string;
  pending: [string, string] | null;
  checking: boolean;
  last_error: string | null;
  /** Version installée depuis le lancement précédent : annonce « Oniris est passé en X ». */
  updated: { version: string; notes: string } | null;
};

export const updateIpc = {
  status: (): Promise<UpdateStatus> => invoke("update_status"),
  checkNow: (): Promise<UpdateStatus> => invoke("update_check_now"),
  restart: (): Promise<void> => invoke("update_restart"),
  whatsNew: (): Promise<void> => invoke("update_whats_new"),
};
export const onUpdate = (cb: (u: UpdateStatus) => void): Promise<UnlistenFn> => listen<UpdateStatus>("oniris://update", (e) => cb(e.payload));

/** Texte d'attente ou d'installation après « Redémarrer » ; `null` hors de ces états. */
export const restartText = (s: UpdatePhase): string | null =>
  s.state === "installing" ? "Installation… Oniris va redémarrer"
    : s.state === "waiting" ? (s.reason === "game" ? "En attente : tu joues" : "En attente : tu parles à IRIS")
      : null;

/** « Réessayer » : relance l'installation si une mise à jour est prête, sinon une vérification. */
export const retry = (s: UpdateStatus): Promise<unknown> => (s.pending ? updateIpc.restart() : updateIpc.checkNow());
