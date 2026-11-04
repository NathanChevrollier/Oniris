import { invoke, isTauri, listen } from "../../lib/ipc";

/** Miroir de `oniris_core::system::SystemStatus` (GPU/VRAM `null` quand PDH est indisponible). */
export type SystemStatus = {
  cpu_pct: number; ram_used_mb: number; ram_total_mb: number;
  gpu_pct: number | null; vram_used_mb: number | null; vram_total_mb: number | null;
  oniris_mb: number; local_ai: string; voice: string;
};

/** Valeurs de la maquette pour l'aperçu navigateur (hors Tauri). */
export const FAKE_STATUS: SystemStatus = {
  cpu_pct: 8, ram_used_mb: 14_100, ram_total_mb: 32_000, gpu_pct: 3, vram_used_mb: 960, vram_total_mb: 16_000,
  oniris_mb: 41, local_ai: "non installée", voice: "non installée",
};

export const systemIpc = {
  snapshot: (): Promise<SystemStatus> => isTauri ? invoke("system_snapshot") : Promise.resolve(FAKE_STATUS),
  /** `oniris://system` n'est émis que panneau ouvert : s'abonner seulement à ce moment. Retourne la désinscription. */
  subscribe: (cb: (s: SystemStatus) => void): (() => void) => {
    if (!isTauri) cb(FAKE_STATUS); // aperçu navigateur
    const un = listen<SystemStatus>("oniris://system", (e) => cb(e.payload));
    return () => { un.then((f) => f()); };
  },
};
