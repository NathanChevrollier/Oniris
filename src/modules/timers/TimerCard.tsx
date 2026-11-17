import { useState, type KeyboardEvent } from "react";
import { AlarmClock, BellRing, X } from "lucide-react";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useToolAction } from "../../lib/toolAction";
import { useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { alarmsIpc, alarmWhen, isAlarmText, mmss, timersIpc, type AlarmsState, type Timer } from "./api";
import "./timers.css";

const UNAVAILABLE = "Minuteurs indisponibles, réessaie.";
const NONE: Timer[] = [];
const NO_ALARMS: AlarmsState = { alarms: [], ringing: [] };
const R = 34;
const CIRC = 2 * Math.PI * R;

/** Minuteurs et alarmes : anneau, alarme qui sonne et saisie fixes ; seule la liste défile. */
export function TimerCard() {
  const open = usePanelStore((s) => s.open);
  const { data } = useTopic<Timer[]>("timers", timersIpc.list, NONE);
  const { data: al } = useTopic<AlarmsState>("timers", alarmsIpc.state, NO_ALARMS);
  const [text, setText] = useState("");
  const { run, error, clear } = useToolAction(UNAVAILABLE);
  const at = Date.now();
  const live = data.filter((t) => t.ends_ms > at);
  useNow(1000, open && live.length > 0); // panneau fermé ou aucun minuteur : aucun tic
  const [first, ...others] = live;

  const onKey = async (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "Enter" || e.nativeEvent.isComposing) return;
    const t = text.trim();
    if (t && (await run(isAlarmText(t) ? alarmsIpc.set(t) : timersIpc.start(t)))) setText("");
  };

  return (
    <Card title="Minuteur" span={3} delay={0.45}
      aside={first && <button type="button" className="tstop" aria-label={`Arrêter ${first.label}`} onClick={() => run(timersIpc.cancel(first.id))}>Arrêter</button>}>
      <div className="tc">
        {al.ringing.length > 0 ? (
          <div className="tring-alarm" role="alert">
            <BellRing size={16} />
            <span>{al.ringing.map((r) => r.label).join(", ")}</span>
            <button type="button" onClick={() => void alarmsIpc.stop().catch(() => {})}>Arrêter</button>
            <button type="button" onClick={() => void alarmsIpc.snooze().catch(() => {})} aria-label="Répéter dans 5 minutes">+5 min</button>
          </div>
        ) : (
          <div className="tring">
            <svg viewBox="0 0 80 80" aria-hidden>
              <circle className="track" cx="40" cy="40" r={R} />
              {first && <circle className="arc" cx="40" cy="40" r={R} strokeDasharray={CIRC}
                strokeDashoffset={CIRC * (1 - Math.min(1, Math.max(0, (first.ends_ms - at) / first.duration_ms)))} />}
            </svg>
            <div className="tmid">
              <b>{first ? mmss(first.ends_ms - at) : "--:--"}</b>
              <small>{first ? first.label : "au repos"}</small>
            </div>
          </div>
        )}
        {(others.length > 0 || al.alarms.length > 0) && (
          <ul className="tlist scroll-area" aria-label="Minuteurs et alarmes">
            {others.map((t) => (
              <li key={`t${t.id}`}>
                <span>{t.label}</span><time>{mmss(t.ends_ms - at)}</time>
                <button type="button" aria-label={`Annuler ${t.label}`} onClick={() => run(timersIpc.cancel(t.id))}><X size={12} /></button>
              </li>
            ))}
            {al.alarms.map((a) => (
              <li key={`a${a.id}`} className="talarm">
                <AlarmClock size={12} aria-hidden="true" />
                <span>{a.label}</span><time>{alarmWhen(a)}</time>
                <button type="button" aria-label={`Supprimer l'alarme ${a.label}`} onClick={() => run(alarmsIpc.cancel(a.id))}><X size={12} /></button>
              </li>
            ))}
          </ul>
        )}
        <div className="tfoot">
          <div className="tquick">
            <button type="button" onClick={() => run(timersIpc.start("5 minutes"))}>5 min</button>
            <button type="button" onClick={() => run(timersIpc.start("25 minutes"))}>25 min</button>
            <button type="button" onClick={() => run(timersIpc.focus())}>Focus</button>
          </div>
          <input className="tinput" value={text} onChange={(e) => { setText(e.target.value); clear(); }} onKeyDown={onKey}
            placeholder="10 min pâtes · alarme 7h" aria-label="Nouveau minuteur" title="Minuteur (« 10 min pâtes ») ou alarme (« alarme 7h », « réveille-moi en semaine à 6h45 »)"
            autoComplete="off" spellCheck={false} />
          {error && <p className="err" role="alert">{error}</p>}
        </div>
      </div>
    </Card>
  );
}
