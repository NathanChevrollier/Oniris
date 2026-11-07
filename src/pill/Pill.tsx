import { useEffect, useRef, useState } from "react";
import { ipc, onPanelState } from "../lib/ipc";
import { useSettings } from "../lib/settings";
import { hhmm, useNow } from "../lib/time";
import { PillTimer } from "../modules/timers/PillTimer";
import { Orb } from "../ui/Orb";
import { onIris } from "../modules/iris/api";
import "../ui/tokens.css";
import "../ui/glass.css";
import "../ui/motion.css";
import "./pill.css";

/** Après une fermeture, le curseur peut se retrouver sur la pilule : ce survol-là ne rouvre pas le panneau. */
const HOVER_REARM_MS = 600;

export function Pill() {
  const settings = useSettings();
  const now = useNow(60_000);
  const [panelOpen, setPanelOpen] = useState(false);
  const [thinking, setThinking] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const hover = useRef<ReturnType<typeof setTimeout>>(undefined);
  const closedAt = useRef(-Infinity);
  const cancelHover = () => clearTimeout(hover.current);

  useEffect(() => {
    const un = onIris((e) => { if (e.type === "state") setThinking(e.state === "thinking"); });
    return () => { un.then((f) => f()); };
  }, []);

  useEffect(() => {
    const un = onPanelState((p) => {
      setPanelOpen(p.open);
      if (!p.open) { closedAt.current = Date.now(); cancelHover(); }
    });
    return () => { un.then((f) => f()); cancelHover(); };
  }, []);

  // La fenêtre suit la largeur du contenu : mesure débouncée, envoyée seulement si elle change.
  useEffect(() => {
    let last = 0;
    let t: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(([e]) => {
      const w = Math.ceil(e.borderBoxSize[0].inlineSize);
      clearTimeout(t);
      t = setTimeout(() => { if (w !== last) { last = w; ipc.pillResize(w); } }, 100);
    });
    ro.observe(root.current!);
    return () => { ro.disconnect(); clearTimeout(t); };
  }, []);

  return (
    <div ref={root} className={panelOpen ? "pill-root open" : "pill-root"}>
      <button type="button" className="island" aria-label="Ouvrir le panneau Oniris"
        onClick={() => { cancelHover(); ipc.panelToggle(); }}
        onMouseEnter={() => {
          if (settings.hover_open && Date.now() - closedAt.current >= HOVER_REARM_MS) hover.current = setTimeout(() => ipc.panelOpen(), settings.hover_delay_ms);
        }}
        onMouseLeave={cancelHover}>
        <span className="time">{hhmm(now)}</span>
        {/* emplacements remplis aux plans 3 (musique) et 4 (mails) ; le minuteur s'affiche à côté de la musique */}
        <PillTimer />
        <span className="slot-music" hidden />
        <span className="slot-mails" hidden />
        <Orb state={thinking ? "thinking" : "idle"} size={24} />
      </button>
    </div>
  );
}
