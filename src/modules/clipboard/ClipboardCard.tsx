import { useEffect, useRef, useState } from "react";
import { ClipboardList, Pin, Search, SearchX, Trash2 } from "lucide-react";
import { Empty } from "../../ui/Empty";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useNow } from "../../lib/time";
import { useFocusCard } from "../../lib/focusCard";
import { usePanelStore } from "../../panel/store";
import { ago } from "../notes/NotesCard";
import { clipboardIpc, type ClipItem } from "./api";
import "../notes/notes.css"; // champ de recherche (.nsearch) et erreur (.err) partagés
import "./clipboard.css";

const UNAVAILABLE = "Presse-papiers indisponible, réessaie.";
const DEBOUNCE_MS = 150;
const NONE: ClipItem[] = [];

/** Nom accessible court (un texte copié peut faire 64 Ko). */
const label = (c: ClipItem) => (c.kind === "image" ? "l'image" : (c.text ?? "").slice(0, 80));

export function ClipboardCard() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  const [query, setQuery] = useState("");
  const [applied, setApplied] = useState("");
  const [error, setError] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);
  useFocusCard("clipboard", search, setQuery);

  useEffect(() => {
    if (query === applied) return; // aucun minuteur au repos
    const t = setTimeout(() => setApplied(query), DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query, applied]);

  const { data } = useTopic<ClipItem[]>("clipboard", () => clipboardIpc.list(applied), NONE, applied);

  const run = (p: Promise<void>) => p.then(() => setError(null), () => setError(UNAVAILABLE));

  return (
    <Card title="Presse-papiers" span={5} delay={0.35}
      aside={<label className="nsearch"><Search size={12} aria-hidden />
        <input ref={search} type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Rechercher"
          aria-label="Rechercher dans le presse-papiers" autoComplete="off" spellCheck={false} /></label>}>
      {error && <p className="err" role="alert">{error}</p>}
      <ul className="clips">
        {data.map((c) => (
          <li key={c.id}>
            <button type="button" className="paste" aria-label={`Coller ${label(c)}`} onClick={() => run(clipboardIpc.paste(c.id))}>
              <p>{c.pinned && <Pin size={11} className="pinmark" aria-label="Épinglée" role="img" />}
                {c.kind === "image" ? <img src={`data:image/png;base64,${c.thumb ?? ""}`} alt="Image copiée" /> : <>{c.text}{c.truncated && "…"}</>}</p>
              <time>{ago(c.created_ms, now)}</time>
            </button>
            <span className="acts">
              <button type="button" aria-label={`${c.pinned ? "Désépingler" : "Épingler"} ${label(c)}`} onClick={() => run(clipboardIpc.pin(c.id, !c.pinned))}><Pin size={13} /></button>
              <button type="button" aria-label={`Supprimer ${label(c)}`} onClick={() => run(clipboardIpc.remove(c.id))}><Trash2 size={13} /></button>
            </span>
          </li>
        ))}
      </ul>
      {data.length === 0 && (applied.trim()
        ? <Empty icon={SearchX} tint="#64d2ff" role="status">Rien ne correspond dans tes copies.</Empty>
        : <Empty icon={ClipboardList} tint="#64d2ff">Copie un texte ou une image : tout atterrit ici.</Empty>)}
    </Card>
  );
}
