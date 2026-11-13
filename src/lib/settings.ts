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

/** « Alt + Z » : texte d'un raccourci, tel que l'utilisateur le tape. */
export const hotkeyText = (h: HotkeySetting) => [h.ctrl && "Ctrl", h.alt && "Alt", h.shift && "Maj", h.win && "Win", h.key === "Space" ? "Espace" : h.key].filter(Boolean).join(" + ");

/** Miroir de `oniris_core::Site` : `url` en http(s) (300), `name` de 1 à 40 caractères, `slow_ms` 300..=10000. */
export type Site = { name: string; url: string; slow_ms: number };

export type Settings = {
  user_name: string;
  hotkey: HotkeySetting;
  hold_threshold_ms: number;
  hover_open: boolean;
  hover_delay_ms: number;
  autostart: boolean;
  /** Vérification et téléchargement automatiques des mises à jour. */
  auto_update: boolean;
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
  /** Identifiants client OAuth (vide = non configuré) : 200 caractères au plus, sans espace. */
  google_client_id: string;
  spotify_client_id: string;
  /** Intervalle de relève des mails, 2..=5 minutes. */
  mail_poll_minutes: number;
  /** Sites surveillés (sondes toutes les 60 s), 30 au plus. */
  sites: Site[];
  /** Chemin de Zenytt.exe (charge du VPS) : vide = détection automatique ; absolu, en .exe, 260 caractères au plus. */
  zenytt_path: string;
  /** Chemin de claude.exe : vide = détection automatique ; absolu, en .exe, 260 caractères au plus. */
  claude_path: string;
  /** Modèle de Claude Code : vide = par défaut ; 100 caractères au plus parmi A-Z a-z 0-9 . _ : / - [ ] */
  iris_claude_model: string;
  /** Commandes PowerShell d'IRIS sans confirmation (préfixes de jetons) : 50 au plus, ASCII, 100 caractères. */
  iris_shell_allowlist: string[];
  /** Dossiers de travail d'IRIS (renommer, déplacer, supprimer sans carte) : 20 au plus, absolus, locaux, jamais protégés. */
  iris_work_dirs: string[];
  /** Modèle Ollama du cerveau local : 1 à 100 caractères parmi a-z 0-9 . _ : / - */
  iris_local_model: string;
  /** IRIS peut appeler Claude d'elle-même ; faux : seulement sur demande explicite, sinon elle le propose. */
  iris_claude_auto: boolean;
  /** Minutes avant que le modèle local quitte la mémoire : 1 à 60. */
  iris_unload_minutes: number;
  /** Personnalité d'IRIS (`{prénom}` est remplacé) : 1 à 2000 caractères. */
  iris_personality: string;
  /** Modèle de transcription : "small" ou "turbo". */
  voice_stt_model: "small" | "turbo";
  /** IRIS répond à voix haute. */
  voice_tts: boolean;
  /** Volume de la voix : 0 à 100. */
  voice_volume: number;
  /** Minutes d'inactivité avant l'arrêt de Whisper : 1 à 30. */
  voice_unload_minutes: number;
  /** Nom du micro : vide = périphérique par défaut ; 200 caractères au plus. */
  voice_input_device: string;
  voice_wake: boolean;
  voice_wake_learning: boolean;
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
  auto_update: true,
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
  google_client_id: "",
  spotify_client_id: "",
  mail_poll_minutes: 3,
  sites: [],
  zenytt_path: "",
  claude_path: "",
  iris_claude_model: "",
  iris_shell_allowlist: ["git status", "Get-ChildItem", "Get-Location", "Get-Date"],
  iris_work_dirs: [],
  iris_local_model: "qwen3:14b",
  iris_claude_auto: false,
  iris_unload_minutes: 30,
  iris_personality: "Tu es IRIS, l'assistante d'Oniris. Tu tutoies {prénom} et l'appelles par son prénom. Ton ton est chaleureux, concis, avec une pointe d'humour discrète ; jamais servile, jamais bavarde. Une ou deux phrases suffisent pour une action simple.",
  voice_stt_model: "small",
  voice_tts: true,
  voice_volume: 80,
  voice_unload_minutes: 3,
  voice_input_device: "",
  voice_wake: false,
  voice_wake_learning: false,
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
