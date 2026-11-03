import { create } from "zustand";
import { isTauri, type PendingAction } from "../lib/ipc";

type PanelStore = {
  open: boolean;
  /** File FIFO des confirmations ; seule la première est affichée. */
  pending: PendingAction[];
  setOpen: (open: boolean) => void;
  enqueue: (p: PendingAction) => void;
  dequeue: (actionId: number) => void;
};

export const usePanelStore = create<PanelStore>()((set) => ({
  // Sous Tauri, la fenêtre démarre cachée ; dans un navigateur (aperçu), on affiche le panneau ouvert.
  open: !isTauri,
  pending: [],
  setOpen: (open) => set({ open }),
  enqueue: (p) => set((s) => (s.pending.some((x) => x.action_id === p.action_id) ? s : { pending: [...s.pending, p] })),
  dequeue: (id) => set((s) => ({ pending: s.pending.filter((x) => x.action_id !== id) })),
}));
