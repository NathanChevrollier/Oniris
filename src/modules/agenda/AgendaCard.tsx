import { useState, type FormEvent } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { agendaIpc, dayBounds, dayKey, type AgendaEvent } from "./api";
import { MonthGrid } from "./MonthGrid";
import { DayList } from "./DayList";
import "./agenda.css";

const MONTHS = ["Janvier", "Février", "Mars", "Avril", "Mai", "Juin", "Juillet", "Août", "Septembre", "Octobre", "Novembre", "Décembre"];
const NOT_FOUND = "Date introuvable — essaie « demain 14h dentiste »";
const UNAVAILABLE = "Agenda indisponible, réessaie.";

type View = { days: number[]; events: AgendaEvent[] };
const EMPTY: View = { days: [], events: [] };

export function AgendaCard() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  // Sans choix de l'utilisateur, le jour affiché suit « aujourd'hui » (y compris après minuit).
  const [picked, setPicked] = useState<Date | null>(null);
  const sel = picked ?? now;
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const year = sel.getFullYear();
  const month = sel.getMonth() + 1;

  const { data } = useTopic<View>("agenda", async () => {
    const [from, to] = dayBounds(sel);
    const [days, events] = await Promise.all([agendaIpc.monthDays(year, month), agendaIpc.between(from, to)]);
    return { days, events };
  }, EMPTY, dayKey(sel));

  const isNow = now.getFullYear() === year && now.getMonth() + 1 === month;
  const go = (delta: number) => {
    const d = new Date(year, month - 1 + delta, 1);
    setPicked(d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth() ? null : d);
  };
  const fail = (out: Awaited<ReturnType<typeof agendaIpc.create>>) =>
    "Failed" in out ? (out.Failed.error.includes("date introuvable") ? NOT_FOUND : out.Failed.error) : null;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (!t) return;
    try {
      const msg = fail(await agendaIpc.create(t));
      setError(msg);
      if (!msg) setText("");
    } catch { setError(UNAVAILABLE); }
  };
  const remove = async (id: number) => {
    try { setError(fail(await agendaIpc.remove(id))); } catch { setError(UNAVAILABLE); }
  };

  return (
    <Card title={MONTHS[month - 1] + (year === now.getFullYear() ? "" : ` ${year}`)} span={3} delay={0.15}
      aside={<span className="nav">
        <button type="button" aria-label="Mois précédent" onClick={() => go(-1)}><ChevronLeft size={15} /></button>
        <button type="button" aria-label="Mois suivant" onClick={() => go(1)}><ChevronRight size={15} /></button>
      </span>}>
      <MonthGrid year={year} month={month} today={isNow ? now.getDate() : null} selected={sel.getDate()}
        withEvents={data.days} onSelect={(day) => setPicked(new Date(year, month - 1, day))} />
      <DayList events={data.events} onDelete={remove} />
      <form className="add" onSubmit={submit}>
        <input value={text} onChange={(e) => { setText(e.target.value); setError(null); }} placeholder="Ajouter…"
          aria-label="Ajouter un événement" autoComplete="off" spellCheck={false} />
        {error && <p className="err" role="alert">{error}</p>}
      </form>
    </Card>
  );
}
