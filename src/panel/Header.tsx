import { UserRound } from "lucide-react";
import { Orb } from "../ui/Orb";
import { formatLongDate, greeting, hhmm, ss, useNow } from "../lib/time";
import { usePanelStore } from "./store";
import { WeatherBadge } from "../modules/weather/WeatherBadge";

/** Horloge isolée : seul composant rafraîchi chaque seconde, et seulement panneau ouvert. */
function Clock() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(1000, open);
  return (
    <div className="t">{hhmm(now)}<span>:{ss(now)}</span></div>
  );
}

export function Header({ name, onAccounts }: { name: string; onAccounts?: () => void }) {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  return (
    <header className="hdr">
      <div className="hello">
        <span>{formatLongDate(now)}</span>
        <h1>{greeting(now, name)}</h1>
        {onAccounts && <button type="button" className="hdr-btn" onClick={onAccounts}><UserRound size={13} />Comptes</button>}
      </div>
      <div className="orb-wrap">
        <Orb state="idle" size={58} />
        <div className="orb-status"><i />Prêt</div>
      </div>
      <div className="clock">
        <Clock />
        <WeatherBadge />
      </div>
    </header>
  );
}
