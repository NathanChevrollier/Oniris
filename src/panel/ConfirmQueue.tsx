import { useEffect } from "react";
import type { PendingAction } from "../lib/ipc";
import { ConfirmCard } from "./ConfirmCard";
import { usePanelStore } from "./store";

/** File FIFO (dans `usePanelStore`) : seule la première confirmation est affichée, les suivantes attendent leur tour. */
export function ConfirmQueue({ subscribe, resolve }: {
  subscribe: (cb: (p: PendingAction) => void) => Promise<() => void>;
  resolve: (actionId: number, approved: boolean) => unknown;
}) {
  const first = usePanelStore((s) => s.pending[0]);
  useEffect(() => {
    const un = subscribe(usePanelStore.getState().enqueue);
    return () => { un.then((f) => f()); };
  }, [subscribe]);
  if (!first) return null;
  return (
    <ConfirmCard pending={first} onResolve={(ok) => {
      resolve(first.action_id, ok);
      usePanelStore.getState().dequeue(first.action_id);
    }} />
  );
}
