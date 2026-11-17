import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Bell, Feather, Pin, Search, SearchX, Trash2 } from "lucide-react";
import { Empty } from "../../ui/Empty";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useToolAction } from "../../lib/toolAction";
import { useFocusCard } from "../../lib/focusCard";
import { useNow } from "../../lib/time";
import { toolIpc, type ToolOutcome } from "../../lib/ipc";
import { usePanelStore } from "../../panel/store";
import { notesIpc, type Note } from "./api";
import "./notes.css";

const UNAVAILABLE = "Notes indisponibles, réessaie.";
const DEBOUNCE_MS = 150;
const UNDO_MS = 6000;
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

/**
 * Champ qui grandit avec son texte jusqu'à son `max-height` CSS, puis seulement là devient défilant.
 * `scrollHeight` exclut les bordures alors que le champ est en `border-box` : sans elles, il manquait 2 px
 * et le champ défilait sans raison.
 */
function useAutoGrow(value: string) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight + el.offsetHeight - el.clientHeight}px`;
    el.style.overflowY = el.scrollHeight > el.clientHeight + 1 ? "auto" : "hidden";
  }, [value]);
  return ref;
}

/** Entrée valide, Maj+Entrée va à la ligne (et rien pendant une composition IME). */
const isSubmit = (e: KeyboardEvent) => e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing;

function NoteItem({ n, now, onRun, onDelete }: {
  n: Note; now: Date; onRun: (call: Promise<ToolOutcome>) => Promise<boolean>; onDelete: (n: Note, keyboard: boolean) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const box = useAutoGrow(draft ?? "");
  const btn = useRef<HTMLButtonElement>(null);
  // Sortie d'édition déjà traitée (Échap, Entrée) : le `blur` qui suit ne réenregistre pas ; `refocus` : sortie au clavier.
  const closed = useRef(false);
  const refocus = useRef(false);
  const editing = draft !== null;
  const [title, ...rest] = n.body.split("\n");
  const preview = rest.join("\n").trim();

  useEffect(() => {
    const el = box.current;
    if (editing && el) { el.focus(); el.setSelectionRange(el.value.length, el.value.length); }
    else if (!editing && refocus.current) { refocus.current = false; btn.current?.focus(); }
  }, [editing, box]);

  const start = () => { closed.current = false; setDraft(n.body); };
  const stop = (keyboard: boolean) => { closed.current = true; refocus.current = keyboard; setDraft(null); };
  const save = async (keyboard: boolean) => {
    if (closed.current) return;
    const t = draft?.trim() ?? "";
    if (!t || t === n.body) { stop(keyboard); return; } // vide ou inchangé : la note reste telle quelle
    closed.current = true;
    if (await onRun(notesIpc.update(n.id, t))) stop(keyboard);
    else closed.current = false; // échec : le texte reste dans le champ
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); stop(true); return; }
    if (isSubmit(e)) { e.preventDefault(); void save(true); }
  };

  return (
    <li className={`${n.pinned ? "pinned" : ""}${editing ? " editing" : ""}`}>
      {editing ? (
        <textarea ref={box} className="ninput nedit" rows={1} value={draft} aria-label={`Modifier ${label(n)}`} spellCheck={false}
          onChange={(e) => setDraft(e.target.value)} onKeyDown={onKey} onBlur={() => void save(false)} />
      ) : (
        <button ref={btn} type="button" className="nbody" title="Modifier" aria-label={`Modifier ${label(n)}`} onClick={start}>
          <span className="ntitle">{n.pinned && <Pin size={11} className="pinmark" aria-label="Épinglée" role="img" />}{title}</span>
          {preview && <span className="nprev">{preview}</span>}
        </button>
      )}
      {!editing && <time dateTime={new Date(n.updated_ms).toISOString()}>{ago(n.updated_ms, now)}</time>}
      {!editing && (
        <span className="acts">
          <button type="button" className={n.pinned ? "on" : undefined} aria-label={`${n.pinned ? "Désépingler" : "Épingler"} ${label(n)}`} title={n.pinned ? "Désépingler" : "Épingler"}
            onClick={() => void onRun(notesIpc.pin(n.id, !n.pinned))}><Pin size={13} /></button>
          <button type="button" aria-label={`En rappel : ${label(n)}`} title="Créer un rappel" onClick={() => void onRun(notesIpc.toReminder(n.id))}><Bell size={13} /></button>
          <button type="button" aria-label={`Supprimer ${label(n)}`} title="Supprimer" onClick={(e) => onDelete(n, e.detail === 0)}><Trash2 size={13} /></button>
        </span>
      )}
    </li>
  );
}

export function NotesCard() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  const [applied, setApplied] = useState("");
  const [undo, setUndo] = useState<number | null>(null);
  const { run, error, clear } = useToolAction(UNAVAILABLE);
  const box = useAutoGrow(text);
  const search = useRef<HTMLInputElement>(null);
  useFocusCard("notes", search, setQuery);

  useEffect(() => {
    if (query === applied) return; // aucun minuteur au repos
    const t = setTimeout(() => setApplied(query), DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query, applied]);

  // Le bandeau « Annuler » disparaît seul ; minuteur seulement tant qu'il est affiché.
  useEffect(() => {
    if (!undo) return;
    const t = setTimeout(() => setUndo(null), UNDO_MS);
    return () => clearTimeout(t);
  }, [undo]);

  const { data } = useTopic<Note[]>("notes", () => notesIpc.list(applied), NONE, applied);

  const onKey = async (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (!isSubmit(e)) return;
    e.preventDefault();
    const t = text.trim();
    if (t && (await run(notesIpc.create(t)))) setText("");
  };
  // Au clavier, le focus passe sur « Annuler » (le bouton qui l'avait disparaît avec la note).
  const undoBtn = useRef<HTMLButtonElement>(null);
  const focusUndo = useRef(false);
  const remove = (n: Note, keyboard: boolean) => void run(notesIpc.remove(n.id).then((o) => {
    if ("Done" in o) { focusUndo.current = keyboard; setUndo(o.Done.action_id); }
    return o;
  }));
  useEffect(() => { if (undo !== null && focusUndo.current) { focusUndo.current = false; undoBtn.current?.focus(); } }, [undo]);
  const cancelDelete = () => {
    if (undo === null) return;
    setUndo(null);
    void run(toolIpc.undo(undo).then((): ToolOutcome => ({ Done: { action_id: undo, output: null } })));
  };

  return (
    <Card title="Notes" span={4} delay={0.3}
      aside={<label className="nsearch"><Search size={12} aria-hidden />
        <input ref={search} type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Rechercher"
          aria-label="Rechercher une note" autoComplete="off" spellCheck={false} /></label>}>
      <div className="nt">
        <div className="ncompose">
          <textarea ref={box} className="ninput" rows={1} value={text} onChange={(e) => { setText(e.target.value); clear(); }}
            onKeyDown={onKey} placeholder="Noter quelque chose…" aria-label="Nouvelle note" aria-describedby="notes-hint" spellCheck={false} />
          <small id="notes-hint" className="nhint">Entrée pour noter · Maj+Entrée pour aller à la ligne</small>
        </div>
        {error && <p className="err" role="alert">{error}</p>}
        <div className="nundo" role="status">
          {undo !== null && <>Note supprimée.<button ref={undoBtn} type="button" onClick={cancelDelete}>Annuler</button></>}
        </div>
        {data.length > 0 ? (
          <ul className="notes scroll-area" aria-label="Notes">
            {data.map((n) => <NoteItem key={n.id} n={n} now={now} onRun={run} onDelete={remove} />)}
          </ul>
        ) : applied.trim()
          ? <Empty icon={SearchX} tint="#ffd60a" role="status">Rien ne correspond, essaie un autre mot.</Empty>
          : <Empty icon={Feather} tint="#ffd60a">Une idée qui passe ? Écris-la au-dessus : elle t'attendra ici.</Empty>}
      </div>
    </Card>
  );
}
