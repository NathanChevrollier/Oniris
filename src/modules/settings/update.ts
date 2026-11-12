import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke, listen } from "../../lib/ipc";

export type UpdateStatus = { current: string; pending: [string, string] | null; checking: boolean; last_error: string | null };
export type UpdateReady = { version: string; notes: string };

export const updateIpc = {
  status: (): Promise<UpdateStatus> => invoke("update_status"),
  checkNow: (): Promise<UpdateStatus> => invoke("update_check_now"),
  restart: (): Promise<void> => invoke("update_restart"),
};
export const onUpdate = (cb: (u: UpdateReady) => void): Promise<UnlistenFn> => listen<UpdateReady>("oniris://update", (e) => cb(e.payload));
