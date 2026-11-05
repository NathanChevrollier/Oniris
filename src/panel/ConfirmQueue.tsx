import { useEffect, useState } from "react";
import type { PendingAction } from "../lib/ipc";
import { ConfirmCard } from "./ConfirmCard";
import { friendlyError } from "../lib/toolAction";
import { usePanelStore } from "./store";

/** Envoi masqué dans le journal puis approuvé après un redémarrage : les arguments ne se relisent plus. */
const failText = (tool: string, raw: string) =>
  friendlyError(raw, tool === "mail.send" && raw.startsWith("arguments invalides") ? "Action expirée, recommence." : "L'action a échoué.");

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
  const open = usePanelStore((s) => s.open);
  // Message passager : disparaît après 6 s ou à la fermeture du panneau.
  useEffect(() => {
    if (!notice) return;
    if (!open) { setNotice(null); return; }
    const t = setTimeout(() => setNotice(null), 6000);
    return () => clearTimeout(t);
  }, [notice, open]);
  const note = notice && <div className="confirm-notice" role="alert">{notice}</div>;
  if (!first) return note;
  return (
    <>
      {note}
      <ConfirmCard pending={first} onResolve={(ok) => {
        setNotice(null);
        // Un échec à l'approbation (ex. envoi expiré après redémarrage) doit se voir.
        const tool = first.tool;
        Promise.resolve(resolve(first.action_id, ok)).then(
          (out) => { const o = out as { Failed?: { error: string } } | undefined;
            if (o?.Failed) setNotice(failText(tool, o.Failed.error)); },
          () => setNotice("L'action a échoué."),
        );
        usePanelStore.getState().dequeue(first.action_id);
      }} />
    </>
  );
}
