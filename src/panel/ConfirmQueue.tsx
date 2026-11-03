import { useEffect, useState } from "react";
import type { PendingAction } from "../lib/ipc";
import { ConfirmCard } from "./ConfirmCard";

/** File FIFO : seule la première confirmation est affichée, les suivantes attendent leur tour. */
export function ConfirmQueue({ subscribe, resolve }: {
  subscribe: (cb: (p: PendingAction) => void) => Promise<() => void>;
  resolve: (actionId: number, approved: boolean) => unknown;
}) {
  const [queue, setQueue] = useState<PendingAction[]>([]);
  useEffect(() => {
    const un = subscribe((p) => setQueue((q) => (q.some((x) => x.action_id === p.action_id) ? q : [...q, p])));
    return () => { un.then((f) => f()); };
  }, [subscribe]);
  const [first] = queue;
  if (!first) return null;
  return (
    <ConfirmCard pending={first} onResolve={(ok) => {
      resolve(first.action_id, ok);
      setQueue((q) => q.filter((x) => x.action_id !== first.action_id));
    }} />
  );
}
