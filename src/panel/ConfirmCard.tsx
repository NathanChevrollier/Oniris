import { useEffect, useRef, useState } from "react";
import type { PendingAction, Risk } from "../lib/ipc";
import { Card } from "../ui/Card";
import { usePanelStore } from "./store";

const RISK: Record<Risk, string> = { Low: "Risque faible", Medium: "Risque moyen", High: "Risque élevé" };

export function ConfirmCard({ pending, onResolve }: { pending: PendingAction; onResolve: (approved: boolean) => void }) {
  const refuse = useRef<HTMLButtonElement>(null);
  // Le focus démarre sur « Refuser » : Entrée n'autorise jamais par accident.
  useEffect(() => refuse.current?.focus(), [pending.action_id]);
  // Garde-fou anti-clic réflexe : « Autoriser » ne s'arme qu'après 500 ms de visibilité continue de l'action
  // (panneau ouvert) ; fermer le panneau désarme, chaque réouverture relance les 500 ms.
  // Armé pour un identifiant précis : aucune frame où une nouvelle action hérite de l'armement de la précédente.
  const open = usePanelStore((s) => s.open);
  const [armedFor, setArmedFor] = useState<number | null>(null);
  useEffect(() => {
    if (!open) { setArmedFor(null); return; }
    const t = setTimeout(() => setArmedFor(pending.action_id), 500);
    return () => clearTimeout(t);
  }, [pending.action_id, open]);
  const armed = open && armedFor === pending.action_id;
  const { summary, details, risk } = pending.text;
  const p = pending.provenance;
  return (
    <Card title="Confirmation" aside={RISK[risk]} span={12}>
      <p>{summary}</p>
      <pre style={{ fontFamily: "ui-monospace, Consolas, monospace", whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: "40vh", overflowY: "auto" }}>{details}</pre>
      {typeof p !== "string" && <p>Demandé à partir de : {p.Untrusted.source}</p>}
      <button type="button" disabled={!armed} onClick={() => onResolve(true)}>Autoriser</button>
      <button type="button" ref={refuse} onClick={() => onResolve(false)}>Refuser</button>
    </Card>
  );
}
