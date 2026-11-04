import { useEffect, useRef, useState, type FormEvent } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { agendaIpc, dayBounds, type AgendaEvent } from "./api";
import { MonthGrid } from "./MonthGrid";
import { DayList } from "./DayList";
import "./agenda.css";

const MONTHS = ["Janvier", "Février", "Mars", "Avril", "Mai", "Juin", "Juillet", "Août", "Septembre", "Octobre", "Novembre", "Décembre"];
const NOT_FOUND = "Date introuvable — essaie « demain 14h dentiste »";

type View = { days: number[]; events: AgendaEvent[] };
const EMPTY: View = { days: [], events: [] };

export function AgendaCard() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  const [sel, setSel] = useState(() => new Date());
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const year = sel.getFullYear();
  const month = sel.getMonth() + 1;

  const { data, reload } = useTopic<View>("agenda", async () => {
    const [from, to] = dayBounds(sel);
    const [days, events] = await Promise.all([agendaIpc.monthDays(year, month), agendaIpc.between(from, to)]);
    return { days, events };
  }, EMPTY);

  // Changer de jour ou de mois recharge (le premier chargement est celui du sujet).
  const first = useRef(true);
  const selKey = sel.toDateString();
  useEffect(() => {
    if (first.current) { first.current = false; return; }
    if (open) reload();
  }, [selKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const isNow = now.getFullYear() === year && now.getMonth() + 1 === month;
  const go = (delta: number) => setSel((s) => {
    const d = new Date(s.getFullYear(), s.getMonth() + delta, 1);
    return d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth() ? new Date(now) : d;
  });

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (!t) return;
    const out = await agendaIpc.create(t);
    if ("Failed" in out) {
      setError(out.Failed.error.includes("date introuvable") ? NOT_FOUND : out.Failed.error);
    } else {
      setError(null);
      setText("");
    }
  };

  return (
    <Card title={MONTHS[month - 1] + (year === now.getFullYear() ? "" : ` ${year}`)} span={3} delay={0.15}
      aside={<span className="nav">
        <button type="button" aria-label="Mois précédent" onClick={() => go(-1)}><ChevronLeft size={15} /></button>
        <button type="button" aria-label="Mois suivant" onClick={() => go(1)}><ChevronRight size={15} /></button>
      </span>}>
      <MonthGrid year={year} month={month} today={isNow ? now.getDate() : null} selected={sel.getDate()}
        withEvents={data.days} onSelect={(day) => setSel(new Date(year, month - 1, day))} />
      <DayList events={data.events} onDelete={(id) => { agendaIpc.remove(id); }} />
      <form className="add" onSubmit={submit}>
        <input value={text} onChange={(e) => { setText(e.target.value); setError(null); }} placeholder="Ajouter…"
          aria-label="Ajouter un événement" autoComplete="off" spellCheck={false} />
        {error && <p className="err" role="alert">{error}</p>}
      </form>
    </Card>
  );
}
