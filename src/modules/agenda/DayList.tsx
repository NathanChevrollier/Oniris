import { X } from "lucide-react";
import { hhmm } from "../../lib/time";
import type { AgendaEvent } from "./api";

export function DayList({ events, onDelete }: { events: AgendaEvent[]; onDelete: (id: number) => void }) {
  if (events.length === 0) return <p className="dl-empty">Rien de prévu</p>;
  return (
    <ul className="dl">
      {events.map((e) => (
        <li key={e.id}>
          <time>{e.all_day ? "Journée" : hhmm(new Date(e.start_ms))}</time>
          <span>{e.title}</span>
          <button type="button" aria-label={`Supprimer ${e.title}`} onClick={() => onDelete(e.id)}><X size={13} /></button>
        </li>
      ))}
    </ul>
  );
}
