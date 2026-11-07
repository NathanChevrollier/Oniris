import { useCallback, useEffect, useRef, useState } from "react";
import { Undo2, X } from "lucide-react";
import { toolIpc, type JournalEntry, type Origin, type Provenance } from "../../lib/ipc";
import { errorText } from "../accounts/api";
import "../accounts/accounts.css";
import "./iris.css";

const originLabel = (o: Origin) => o === "User" ? "toi" : o === "Mcp" ? "MCP" : o.Assistant.brain === "Claude" ? "Claude" : "IRIS local";
const provenanceLabel = (p: Provenance) => p === "User" ? null : `contenu non fiable : ${p.Untrusted.source}`;
/** JSON abrégé, rendu comme texte (jamais du HTML). */
const brief = (v: unknown) => { const j = JSON.stringify(v) ?? ""; return j.length > 120 ? `${j.slice(0, 119)}…` : j; };
const when = (ms: number) => new Date(ms).toLocaleString("fr-FR", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });

/** Feuille « Journal » : actions menées, avec annulation. Rafraîchie à l'ouverture et après chaque action. */
export function JournalSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [list, setList] = useState<JournalEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLElement>(null);
  const load = useCallback(() => { toolIpc.journal(100).then(setList, () => {}); }, []);
  useEffect(() => { if (open) { root.current?.focus(); load(); } }, [open, load]);
  if (!open) return null;
  const undo = (id: number) => toolIpc.undo(id).then(() => setError(null), (e) => setError(errorText(e, "Annulation impossible."))).finally(load);
  return (
    <section ref={root} tabIndex={-1} className="acc-sheet" role="dialog" aria-modal="true" aria-label="Journal"
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      <header className="acc-head"><h2>Journal</h2>
        <button type="button" className="acc-icon" aria-label="Fermer le journal" onClick={onClose}><X size={16} /></button></header>
      <div className="acc-body">
        {error && <p className="err" role="alert">{error}</p>}
        {list.length === 0 ? <p className="acc-empty">Aucune action pour l'instant.</p> : (
          <ul className="acc-list">{list.map((j) => {
            const prov = provenanceLabel(j.provenance);
            return (
              <li key={j.id} className="acc-row">
                <div className="acc-main">
                  <span className="acc-label">{j.tool}<small>{when(j.ts_ms)} · {originLabel(j.origin)} · {j.status}</small></span>
                  {j.undoable && <button type="button" className="acc-btn" aria-label={`Annuler ${j.tool}`} onClick={() => void undo(j.id)}><Undo2 size={13} />Annuler</button>}
                </div>
                {prov && <p className="jr-prov">{prov}</p>}
                {j.result != null && <code className="jr-res">{brief(j.result)}</code>}
              </li>
            );
          })}</ul>
        )}
      </div>
    </section>
  );
}
