import { Coffee, X } from "lucide-react";
import { Empty } from "../../ui/Empty";
import { hhmm } from "../../lib/time";
import type { AgendaEvent } from "./api";

export function DayList({ events, onDelete }: { events: AgendaEvent[]; onDelete: (id: number) => void }) {
  if (events.length === 0) return <Empty icon={Coffee} tint="#ffd60a">Rien de prévu, journée libre.</Empty>;
  return (
    <ul className="dl">
      {events.map((e) => (
        <li key={e.key}>
          <time>{e.all_day ? "Journée" : hhmm(new Date(e.start_ms))}</time>
          {e.source === "google" && <i className="dot" style={{ background: e.color ?? undefined }} />}
          <span>{e.title}</span>
          {!e.readonly && <button type="button" aria-label={`Supprimer ${e.title}`} onClick={() => onDelete(e.id)}><X size={13} /></button>}
        </li>
      ))}
    </ul>
  );
}
