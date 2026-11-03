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
};
