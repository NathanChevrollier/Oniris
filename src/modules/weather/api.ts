import { invoke, isTauri } from "../../lib/ipc";

/** Miroir de `oniris_core::weather::Weather`. `icon` : nom d'icône Lucide. */
export type Weather = {
  place: string; temp_c: number; code: number; label: string; icon: string;
  today_min: number; today_max: number; rain_from_hour: number | null; fetched_ms: number;
};

/** Valeurs de la maquette pour l'aperçu navigateur (hors Tauri). */
export const fakeWeather = (): Weather => ({
  place: "Paris", temp_c: 12, code: 61, label: "Pluie faible", icon: "CloudRain",
  today_min: 8, today_max: 13, rain_from_hour: 18, fetched_ms: Date.now(),
});

export const weatherIpc = {
  /** Cache seulement : aucune requête réseau. */
  get: (): Promise<Weather | null> => isTauri ? invoke("weather_get") : Promise.resolve(fakeWeather()),
};
