import { useState, type FormEvent } from "react";
import { Check } from "lucide-react";
import { useTopic } from "../../lib/data";
import { hhmm, useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { remindersIpc, type Reminder } from "./api";
import "./reminders.css";

const UNAVAILABLE = "Rappels indisponibles, réessaie.";
const SOON_MS = 2 * 3_600_000;
const NONE: Reminder[] = [];

/** Heure si c'est aujourd'hui, sinon jour de la semaine abrégé (« jeu. »). */
function dueLabel(due: Date, now: Date) {
  return due.toDateString() === now.toDateString() ? hhmm(due) : due.toLocaleDateString("fr-FR", { weekday: "short" });
}

export function ReminderList() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const { data } = useTopic<Reminder[]>("reminders", remindersIpc.list, NONE, now.toDateString());

  const run = async (p: ReturnType<typeof remindersIpc.create>) => {
    try {
      const out = await p;
      const msg = "Failed" in out ? out.Failed.error : null;
      setError(msg);
      return !msg;
    } catch { setError(UNAVAILABLE); return false; }
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (t && (await run(remindersIpc.create(t)))) setText("");
  };

  return (
    <div className="rems">
      {data.map((r) => {
        const due = r.due_ms == null ? null : new Date(r.due_ms);
        const soon = due != null && due.getTime() - now.getTime() <= SOON_MS;
        return (
          <div className="rem" key={r.id}>
            <button type="button" className="ck" aria-label={`Terminer ${r.text}`} onClick={() => run(remindersIpc.complete(r.id, true))}>
              <Check size={11} />
            </button>
            <span>{r.text}</span>
            {due && <small className={soon ? "soon" : undefined}>{dueLabel(due, now)}</small>}
          </div>
        );
      })}
      <form className="add" onSubmit={submit}>
        <input value={text} onChange={(e) => { setText(e.target.value); setError(null); }} placeholder="Nouveau rappel…"
          aria-label="Nouveau rappel" autoComplete="off" spellCheck={false} />
        {error && <p className="err" role="alert">{error}</p>}
      </form>
    </div>
  );
}
