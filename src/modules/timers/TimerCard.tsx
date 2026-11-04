import { useState, type KeyboardEvent } from "react";
import { X } from "lucide-react";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { mmss, timersIpc, type Timer } from "./api";
import "./timers.css";

const UNAVAILABLE = "Minuteurs indisponibles, réessaie.";
const NONE: Timer[] = [];
const R = 34;
const CIRC = 2 * Math.PI * R;

export function TimerCard() {
  const open = usePanelStore((s) => s.open);
  const { data } = useTopic<Timer[]>("timers", timersIpc.list, NONE);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const at = Date.now();
  const live = data.filter((t) => t.ends_ms > at);
  useNow(1000, open && live.length > 0); // panneau fermé ou aucun minuteur : aucun tic
  const [first, ...others] = live;

  const run = async (p: ReturnType<typeof timersIpc.start>) => {
    try {
      const out = await p;
      const msg = "Failed" in out ? out.Failed.error : null;
      setError(msg);
      return !msg;
    } catch { setError(UNAVAILABLE); return false; }
  };
  const onKey = async (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "Enter" || e.nativeEvent.isComposing) return;
    const t = text.trim();
    if (t && (await run(timersIpc.start(t)))) setText("");
  };

  return (
    <Card title="Minuteur" span={3} delay={0.45}>
      <div className="tring">
        <svg viewBox="0 0 80 80" aria-hidden>
          <circle className="track" cx="40" cy="40" r={R} />
          {first && <circle className="arc" cx="40" cy="40" r={R} strokeDasharray={CIRC}
            strokeDashoffset={CIRC * (1 - Math.min(1, Math.max(0, (first.ends_ms - at) / first.duration_ms)))} />}
        </svg>
        <div className="tmid">
          <b>{first ? mmss(first.ends_ms - at) : "--:--"}</b>
          <small>{first ? first.label : "Aucun"}</small>
        </div>
      </div>
      {others.length > 0 && (
        <ul className="tlist">
          {others.map((t) => (
            <li key={t.id}>
              <span>{t.label}</span><time>{mmss(t.ends_ms - at)}</time>
              <button type="button" aria-label={`Annuler ${t.label}`} onClick={() => run(timersIpc.cancel(t.id))}><X size={12} /></button>
            </li>
          ))}
        </ul>
      )}
      {first && <button type="button" className="tstop" onClick={() => run(timersIpc.cancel(first.id))}>Annuler {first.label}</button>}
      <div className="tquick">
        <button type="button" onClick={() => run(timersIpc.start("5 minutes"))}>5 min</button>
        <button type="button" onClick={() => run(timersIpc.start("25 minutes"))}>25 min</button>
        <button type="button" onClick={() => run(timersIpc.focus())}>Focus</button>
      </div>
      <input className="tinput" value={text} onChange={(e) => { setText(e.target.value); setError(null); }} onKeyDown={onKey}
        placeholder="Ex. 10 min pâtes" aria-label="Nouveau minuteur" autoComplete="off" spellCheck={false} />
      {error && <p className="err" role="alert">{error}</p>}
    </Card>
  );
}
