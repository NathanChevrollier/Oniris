import type { CSSProperties } from "react";
import { hhmm, nowPosition, useNow } from "../lib/time";
import { usePanelStore } from "./store";

export type TimelineItem = { start: Date; end?: Date; label: string; kind: "event" | "reminder"; color?: string; readonly?: boolean };

/** Marqueur « maintenant », isolé : re-rendu à la minute, panneau ouvert uniquement. */
function NowMarker({ startHour, endHour }: { startHour: number; endHour: number }) {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  return <div className="now" style={{ left: `calc(${nowPosition(now, startHour, endHour) * 100}% - 1px)` }} />;
}

export function Timeline({ startHour, endHour, items }: { startHour: number; endHour: number; items: TimelineItem[] }) {
  const pct = (d: Date) => nowPosition(d, startHour, endHour) * 100;
  const hours = Array.from({ length: endHour - startHour }, (_, i) => startHour + i);
  return (
    <div className="tl">
      <div className="hours">{hours.map((h) => <div key={h}>{h}h</div>)}</div>
      {items.map((it, i) => {
        const color = it.color ?? (it.kind === "reminder" ? "var(--yellow)" : "var(--blue)");
        if (it.kind === "reminder")
          return <div key={i} className="rem" title={it.label} style={{ left: `calc(${pct(it.start)}% - 4px)`, "--c": color } as CSSProperties} />;
        const end = it.end ?? new Date(it.start.getTime() + 3_600_000); // sans fin : une heure par défaut
        return (
          <div key={i} className="ev" title={it.label} data-readonly={it.readonly || undefined}
            style={{ left: `${pct(it.start)}%`, width: `calc(${pct(end) - pct(it.start)}% - 4px)`, "--c": color } as CSSProperties}>
            {it.label} <small>{hhmm(it.start)}{it.end && `–${hhmm(it.end)}`}</small>
          </div>
        );
      })}
      <NowMarker startHour={startHour} endHour={endHour} />
    </div>
  );
}
