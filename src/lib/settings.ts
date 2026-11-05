import { useEffect, useState } from "react";
import { ipc, onSettings } from "./ipc";

/** Miroir de `oniris_core::Settings` (noms snake_case tels que sérialisés par serde). */
export type HotkeySetting = {
  alt: boolean;
  ctrl: boolean;
  shift: boolean;
  win: boolean;
  /** "Space", "A".."Z" ou "F1".."F12" */
  key: string;
};

export type Settings = {
  user_name: string;
  hotkey: HotkeySetting;
  hold_threshold_ms: number;
  hover_open: boolean;
  hover_delay_ms: number;
  autostart: boolean;
  hide_pill_in_fullscreen: boolean;
  timeline_start_hour: number;
  timeline_end_hour: number;
  reduce_motion: boolean;
  focus_minutes: number;
  break_minutes: number;
  clipboard_enabled: boolean;
  /** 1..=90 */
  clipboard_retention_days: number;
  /** 20..=1000 */
  clipboard_max_items: number;
  /** Ville de la météo, 80 caractères au plus ; vide = météo désactivée. */
  weather_city: string;
  /** Dossiers indexés par la recherche (vide = dossier utilisateur) ; 20 chemins absolus au plus, 260 caractères. */
  search_roots: string[];
  /** Dossiers de projets, indexés en plus ; mêmes limites. */
  projects_roots: string[];
  /** Invites de « Reprendre » (Claude Code) : non vides, 2000 caractères au plus. */
  resume_prompt_normal: string;
  resume_prompt_orchestrator: string;
};

const RESUME_NORMAL = "Reprends là où tu en étais sur ce projet. Appuie-toi sur le plan de développement et l'historique git. Commence par me dire où on en est.";

/** Miroir de `Settings::default()` côté Rust : valeurs affichées avant la réponse de `settings_get`. */
export const DEFAULT_SETTINGS: Settings = {
  user_name: "",
  hotkey: { alt: true, ctrl: false, shift: false, win: false, key: "Space" },
  hold_threshold_ms: 250,
  hover_open: true,
  hover_delay_ms: 350,
  autostart: true,
  hide_pill_in_fullscreen: true,
  timeline_start_hour: 8,
  timeline_end_hour: 23,
  reduce_motion: false,
  focus_minutes: 25,
  break_minutes: 5,
  clipboard_enabled: true,
  clipboard_retention_days: 7,
  clipboard_max_items: 200,
  weather_city: "",
  search_roots: [],
  projects_roots: [],
  resume_prompt_normal: RESUME_NORMAL,
  resume_prompt_orchestrator: `${RESUME_NORMAL} Travaille en mode orchestrateur, comme décrit dans mon CLAUDE.md global : graphify pour t'orienter, sous-agents haiku/sonnet/opus selon la tâche, parallélisation des tâches indépendantes, et relecture avant intégration.`,
};

/** Paramètres courants : chargés au montage, puis suivis via `oniris://settings`. */
export function useSettings(): Settings {
  const [s, setS] = useState(DEFAULT_SETTINGS);
  useEffect(() => {
    ipc.getSettings().then(setS, () => {});
    const un = onSettings(setS);
    return () => { un.then((f) => f()); };
  }, []);
  // Réglage « réduire les animations » : même effet que prefers-reduced-motion (motion.css).
  useEffect(() => { document.documentElement.classList.toggle("reduce-motion", s.reduce_motion); }, [s.reduce_motion]);
  return s;
}
