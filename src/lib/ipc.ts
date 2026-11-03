import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type PanelState = { open: boolean; at_ms: number };

export const ipc = {
  panelOpen: () => invoke<void>("panel_open"),
  panelClose: () => invoke<void>("panel_close"),
  panelToggle: () => invoke<void>("panel_toggle"),
  pillResize: (width: number) => invoke<void>("pill_resize", { width }),
};

export const onPanelState = (cb: (p: PanelState) => void): Promise<UnlistenFn> =>
  listen<PanelState>("oniris://panel-state", (e) => cb(e.payload));
