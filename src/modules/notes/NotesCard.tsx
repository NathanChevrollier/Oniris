import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Bell, Feather, Pin, Search, SearchX, Trash2 } from "lucide-react";
import { Empty } from "../../ui/Empty";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useToolAction } from "../../lib/toolAction";
import { useFocusCard } from "../../lib/focusCard";
import { useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { notesIpc, type Note } from "./api";
import "./notes.css";

const UNAVAILABLE = "Notes indisponibles, réessaie.";
const DEBOUNCE_MS = 150;
const NONE: Note[] = [];
/** Libellé lisible par un lecteur d'écran : la note entière serait lue à chaque bouton. */
const label = (n: Note) => n.body.slice(0, 80);

/** « à l'instant », « 5 min », « 3 h », « hier », « 12 nov. ». */
export function ago(ms: number, now: Date): string {
  const min = Math.floor((now.getTime() - ms) / 60_000);
  if (min < 1) return "à l'instant";
  if (min < 60) return `${min} min`;
  if (min < 24 * 60) return `${Math.floor(min / 60)} h`;
  const d = new Date(ms);
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  return d.toDateString() === yesterday.toDateString() ? "hier" : d.toLocaleDateString("fr-FR", { day: "numeric", month: "short" });
}

export function NotesCard() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  const [applied, setApplied] = useState("");
  const { run, error, clear } = useToolAction(UNAVAILABLE);
  const box = useRef<HTMLTextAreaElement>(null);
  const search = useRef<HTMLInputElement>(null);
  useFocusCard("notes", search, setQuery);

  useEffect(() => {
    if (query === applied) return; // aucun minuteur au repos
    const t = setTimeout(() => setApplied(query), DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query, applied]);

  const { data } = useTopic<Note[]>("notes", () => notesIpc.list(applied), NONE, applied);

  // Champ auto-extensible ; la hauteur maximale (6 lignes) est bornée en CSS.
  useEffect(() => {
    const el = box.current;
    if (el) { el.style.height = "auto"; el.style.height = `${el.scrollHeight}px`; }
  }, [text]);

  const onKey = async (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    const t = text.trim();
    if (t && (await run(notesIpc.create(t)))) setText("");
  };

  return (
    <Card title="Notes" span={4} delay={0.3}
      aside={<label className="nsearch"><Search size={12} aria-hidden />
        <input ref={search} type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Rechercher"
          aria-label="Rechercher une note" autoComplete="off" spellCheck={false} /></label>}>
      <textarea ref={box} className="ninput" rows={1} value={text} onChange={(e) => { setText(e.target.value); clear(); }}
        onKeyDown={onKey} placeholder="Noter quelque chose…" aria-label="Nouvelle note" spellCheck={false} />
      {error && <p className="err" role="alert">{error}</p>}
      <ul className="notes">
        {data.map((n) => (
          <li key={n.id}>
            <p>{n.pinned && <Pin size={11} className="pinmark" aria-label="Épinglée" role="img" />}{n.body}</p>
            <time>{ago(n.updated_ms, now)}</time>
            <span className="acts">
              <button type="button" aria-label={`${n.pinned ? "Désépingler" : "Épingler"} ${label(n)}`} onClick={() => run(notesIpc.pin(n.id, !n.pinned))}><Pin size={13} /></button>
              <button type="button" aria-label={`En rappel : ${label(n)}`} onClick={() => run(notesIpc.toReminder(n.id))}><Bell size={13} /></button>
              <button type="button" aria-label={`Supprimer ${label(n)}`} onClick={() => run(notesIpc.remove(n.id))}><Trash2 size={13} /></button>
            </span>
          </li>
        ))}
      </ul>
      {data.length === 0 && (applied.trim()
        ? <Empty icon={SearchX} tint="#ffd60a" role="status">Rien ne correspond, essaie un autre mot.</Empty>
        : <Empty icon={Feather} tint="#ffd60a">Une idée qui passe ? Note-la, elle t'attendra ici.</Empty>)}
    </Card>
  );
}
