import { useEffect, useState } from "react";
import type { PendingAction } from "../lib/ipc";
import { ConfirmCard } from "./ConfirmCard";
import { friendlyError } from "../lib/toolAction";
import { usePanelStore } from "./store";

/** File FIFO (dans `usePanelStore`) : seule la première confirmation est affichée, les suivantes attendent leur tour. */
export function ConfirmQueue({ subscribe, resolve }: {
  subscribe: (cb: (p: PendingAction) => void) => Promise<() => void>;
  resolve: (actionId: number, approved: boolean) => unknown;
}) {
  const first = usePanelStore((s) => s.pending[0]);
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => {
    const un = subscribe(usePanelStore.getState().enqueue);
    return () => { un.then((f) => f()); };
  }, [subscribe]);
  const note = notice && <p className="err" role="alert">{notice}</p>;
  if (!first) return note;
  return (
    <>
      {note}
      <ConfirmCard pending={first} onResolve={(ok) => {
        setNotice(null);
        // Un échec à l'approbation (ex. envoi expiré après redémarrage) doit se voir.
        Promise.resolve(resolve(first.action_id, ok)).then(
          (out) => { const o = out as { Failed?: { error: string } } | undefined; if (o?.Failed) setNotice(friendlyError(o.Failed.error, "L'action a échoué.")); },
          () => setNotice("L'action a échoué."),
        );
        usePanelStore.getState().dequeue(first.action_id);
      }} />
    </>
  );
}
