import { BookText, Settings, Sparkles, UserRound } from "lucide-react";
import { IrisOrb, IrisStatusLine } from "../modules/iris/IrisReply";
import { useVoice, voiceIpc } from "../modules/voice/api";
import { formatLongDate, greeting, hhmm, ss, useNow } from "../lib/time";
import { usePanelStore } from "./store";
import { WeatherBadge } from "../modules/weather/WeatherBadge";
import { QuickControls } from "./QuickControls";

/** Horloge isolée : seul composant rafraîchi chaque seconde, et seulement panneau ouvert. */
function Clock() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(1000, open);
  return (
    <div className="t">{hhmm(now)}<span>:{ss(now)}</span></div>
  );
}

/** Rend le focus au bouton d'en-tête qui a ouvert la feuille (après le rendu où le fond redevient actif). */
export const focusHeaderButton = (sheet: string) => setTimeout(() => document.querySelector<HTMLElement>(`.hdr-btn[data-sheet="${sheet}"]`)?.focus(), 0);

export function Header({ name, onAccounts, onIris, onJournal, onSettings }: { name: string; onAccounts?: () => void; onIris?: () => void; onJournal?: () => void; onSettings?: () => void }) {
  const open = usePanelStore((s) => s.open);
  const talk = useVoice().phase === "listening" ? "Arrêter l'écoute" : "Parler à IRIS";
  const now = useNow(60_000, open);
  return (
    <header className="hdr">
      <div className="hello">
        <span>{formatLongDate(now)}</span>
        <h1>{greeting(now, name)}</h1>
        <nav className="hdr-actions" aria-label="Raccourcis">
          {onAccounts && <button type="button" className="hdr-btn" data-sheet="accounts" onClick={onAccounts}><UserRound size={13} />Comptes</button>}
          {onIris && <button type="button" className="hdr-btn" data-sheet="iris" onClick={onIris}><Sparkles size={13} />IRIS</button>}
          {onJournal && <button type="button" className="hdr-btn" data-sheet="journal" onClick={onJournal}><BookText size={13} />Journal</button>}
          {onSettings && <button type="button" className="hdr-btn" data-sheet="settings" onClick={onSettings}><Settings size={13} />Réglages</button>}
        </nav>
      </div>
      <div className="orb-wrap">
        <button type="button" className="orb-btn" aria-label={talk} title={talk} onClick={() => void voiceIpc.tap().catch(() => {})}><IrisOrb size={58} /></button>
        <IrisStatusLine onFix={onIris} />
      </div>
      <div className="clock">
        <Clock />
        <WeatherBadge />
        <QuickControls />
      </div>
    </header>
  );
}
