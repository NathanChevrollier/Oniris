import { useEffect, useRef } from "react";
import type { PendingAction, Risk } from "../lib/ipc";
import { Card } from "../ui/Card";

const RISK: Record<Risk, string> = { Low: "Risque faible", Medium: "Risque moyen", High: "Risque élevé" };

export function ConfirmCard({ pending, onResolve }: { pending: PendingAction; onResolve: (approved: boolean) => void }) {
  const refuse = useRef<HTMLButtonElement>(null);
  // Le focus démarre sur « Refuser » : Entrée n'autorise jamais par accident.
  useEffect(() => refuse.current?.focus(), [pending.action_id]);
  const { summary, details, risk } = pending.text;
  const p = pending.provenance;
  return (
    <Card title="Confirmation" aside={RISK[risk]} span={12}>
      <p>{summary}</p>
      <pre style={{ fontFamily: "ui-monospace, Consolas, monospace", whiteSpace: "pre-wrap" }}>{details}</pre>
      {typeof p !== "string" && <p>Demandé à partir de : {p.Untrusted.source}</p>}
      <button type="button" onClick={() => onResolve(true)}>Autoriser</button>
      <button type="button" ref={refuse} onClick={() => onResolve(false)}>Refuser</button>
    </Card>
  );
}
