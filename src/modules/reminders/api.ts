import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

/** Miroir de `oniris_core::reminders::Reminder`. */
export type Reminder = { id: number; text: string; due_ms: number | null; done: boolean };

/** Rappels factices pour l'aperçu navigateur (hors Tauri). */
function fakeReminders(): Reminder[] {
  const inMs = (min: number) => Date.now() + min * 60_000;
  return [
    { id: 1, text: "Appeler la banque", due_ms: inMs(90), done: false },
    { id: 2, text: "Renouveler le domaine", due_ms: inMs(3 * 24 * 60), done: false },
    { id: 3, text: "Penser au pain", due_ms: null, done: false },
    { id: 4, text: "Sauvegarde VPS", due_ms: inMs(-120), done: true },
  ];
}

const fakeDone: ToolOutcome = { Done: { action_id: 0, output: {} } };

export const remindersIpc = {
  /** Ouverts, puis cochés aujourd'hui (barrés jusqu'à minuit). */
  list: (): Promise<Reminder[]> => isTauri ? invoke("reminders_list") : Promise.resolve(fakeReminders()),
  create: (text: string): Promise<ToolOutcome> => isTauri ? invoke("reminders_create", { text }) : Promise.resolve(fakeDone),
  complete: (id: number, done: boolean): Promise<ToolOutcome> => isTauri ? invoke("reminders_complete", { id, done }) : Promise.resolve(fakeDone),
  remove: (id: number): Promise<ToolOutcome> => isTauri ? invoke("reminders_delete", { id }) : Promise.resolve(fakeDone),
};
