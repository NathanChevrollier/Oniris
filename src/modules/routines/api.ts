import { invoke } from "../../lib/ipc";

/** Miroir de `oniris_core::routines::Routine`. */
export type Step = { tool: string; args: Record<string, unknown>; blocking: boolean };
export type Routine = { name: string; phrases: string[]; steps: Step[] };
/** Miroir de `oniris_core::routines::Report` (seuls les champs lus ici). */
export type RunReport = { summary?: string; pending?: number };

export const routinesIpc = {
  list: (): Promise<Routine[]> => invoke("routines_list"),
  /** Remplace la liste entière ; rend la liste nettoyée par Rust, ou rejette avec la raison. */
  save: (routines: Routine[]): Promise<Routine[]> => invoke("routines_save", { routines }),
  run: (name: string): Promise<RunReport> => invoke("routines_run", { name }),
};
