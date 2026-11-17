import { Coffee, X } from "lucide-react";
import { Empty } from "../../ui/Empty";
import { hhmm } from "../../lib/time";
import type { AgendaEvent, EventSource } from "./api";

const label = (s: EventSource) => (s.account ? `${s.calendar} (${s.account})` : s.calendar);

/** Pastilles des calendriers d'origine (plusieurs après fusion des doublons). */
function Sources({ e }: { e: AgendaEvent }) {
  const list = e.sources?.length ? e.sources : e.source !== "local" ? [{ calendar: e.calendar ?? "Google", account: null, color: e.color }] : [];
  if (list.length === 0) return null;
  const text = list.length > 1 ? `Dans ${list.length} calendriers : ${list.map(label).join(", ")}` : `Calendrier : ${label(list[0])}`;
  return (
    <span className="dl-src" title={text} aria-label={text} role="img">
      {list.slice(0, 4).map((s, i) => <i key={i} className="dot" style={{ background: s.color ?? undefined }} />)}
    </span>
  );
}

export function DayList({ events, onDelete, loading = false }: { events: AgendaEvent[]; onDelete: (id: number) => void; loading?: boolean }) {
  if (loading) return <p className="dl-wait" role="status">Chargement de la journée…</p>;
  if (events.length === 0) return <Empty icon={Coffee} tint="#ffd60a">Rien de prévu, journée libre.</Empty>;
  return (
    <ul className="dl">
      {events.map((e) => (
        <li key={e.key}>
          <time>{e.all_day ? "Journée" : hhmm(new Date(e.start_ms))}</time>
          <Sources e={e} />
          <span className="dl-t">{e.title}{e.calendar && <small>{e.calendar}</small>}</span>
          {!e.readonly && <button type="button" aria-label={`Supprimer ${e.title}`} onClick={() => onDelete(e.id)}><X size={13} /></button>}
        </li>
      ))}
    </ul>
  );
}
