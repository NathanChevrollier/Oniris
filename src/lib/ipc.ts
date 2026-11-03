import { invoke } from "@tauri-apps/api/core";
import type { Settings } from "./settings";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type PanelState = { open: boolean; at_ms: number };

export const ipc = {
  panelOpen: () => invoke<void>("panel_open"),
  panelClose: () => invoke<void>("panel_close"),
  panelToggle: () => invoke<void>("panel_toggle"),
  pillResize: (width: number) => invoke<void>("pill_resize", { width }),
  getSettings: () => invoke<Settings>("settings_get"),
  saveSettings: (settings: Settings) => invoke<void>("settings_save", { settings }),
};

export const onPanelState = (cb: (p: PanelState) => void): Promise<UnlistenFn> =>
  listen<PanelState>("oniris://panel-state", (e) => cb(e.payload));

export const onSettings = (cb: (s: Settings) => void): Promise<UnlistenFn> =>
  listen<Settings>("oniris://settings", (e) => cb(e.payload));

// Miroirs des types Rust de oniris_core::tools (représentation serde par défaut).
export type Risk = "Low" | "Medium" | "High";
export type Provenance = "User" | { Untrusted: { source: string } };
export type Origin = "User" | "Mcp" | { Assistant: { brain: "Local" | "Claude" } };
export type ConfirmationText = { summary: string; details: string; risk: Risk };
export type PendingAction = {
  action_id: number; tool: string; text: ConfirmationText; provenance: Provenance; expires_at_ms: number;
};
export type JournalEntry = {
  id: number; ts_ms: number; tool: string; args: unknown; origin: Origin; provenance: Provenance;
  status: string; result: unknown | null; undoable: boolean;
};
export type ToolOutcome =
  | { Done: { action_id: number; output: unknown } }
  | { NeedsConfirmation: PendingAction }
  | { Rejected: { action_id: number } }
  | { Failed: { action_id: number; error: string } };

export const toolIpc = {
  resolve: (actionId: number, approved: boolean) => invoke<ToolOutcome>("tool_resolve", { actionId, approved }),
  undo: (actionId: number) => invoke<void>("tool_undo", { actionId }),
  journal: (limit: number) => invoke<JournalEntry[]>("journal_list", { limit }),
};

export const onConfirm = (cb: (p: PendingAction) => void): Promise<UnlistenFn> =>
  listen<PendingAction>("oniris://confirm", (e) => cb(e.payload));
