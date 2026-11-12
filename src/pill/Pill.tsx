import { useEffect, useRef, useState } from "react";
import { ipc, onPanelState } from "../lib/ipc";
import { useSettings } from "../lib/settings";
import { hhmm, useNow } from "../lib/time";
import { PillTimer } from "../modules/timers/PillTimer";
import { Orb } from "../ui/Orb";
import { onIris } from "../modules/iris/api";
import { onVoice, orbFor, useVoice, voiceIpc } from "../modules/voice/api";
import { onUpdate, updateIpc } from "../modules/settings/update";
import { PillToast } from "../modules/voice/PillToast";
import "../ui/tokens.css";
import "../ui/glass.css";
import "../ui/motion.css";
import "./pill.css";

/** Après une fermeture, le curseur peut se retrouver sur la pilule : ce survol-là ne rouvre pas le panneau. */
const HOVER_REARM_MS = 600;

/** Seul abonné au niveau du micro (dizaines d'événements par seconde) : la pilule autour ne se redessine pas. */
export function VoiceOrb({ thinking }: { thinking: boolean }) {
  const voice = useVoice();
  return <Orb state={orbFor(voice.phase, thinking)} size={24} level={voice.level} />;
}

const ARMED_MS = 5000;
const FALSE_WAKE_MS = 10_000;

/** Boutons du mode apprentissage : « Faux réveil » seulement dans les 10 s qui suivent un réveil par la voix. */
function LearnButtons() {
  const [armed, setArmed] = useState(false);
  const [recent, setRecent] = useState(false);
  const armT = useRef<ReturnType<typeof setTimeout>>(undefined);
  const wakeT = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => {
    const un = onVoice((e) => {
      if (!e.by_wake) return;
      setRecent(true);
      clearTimeout(wakeT.current);
      wakeT.current = setTimeout(() => setRecent(false), FALSE_WAKE_MS);
    });
    return () => { void un.then((f) => f()); clearTimeout(armT.current); clearTimeout(wakeT.current); };
  }, []);
  return (
    <>
      <button type="button" className="pill-learn" aria-label="Je vais dire Iris" data-armed={armed || undefined}
        onClick={() => {
          void voiceIpc.expectWake();
          setArmed(true);
          clearTimeout(armT.current);
          armT.current = setTimeout(() => setArmed(false), ARMED_MS);
        }}>Iris ?</button>
      {recent && <button type="button" className="pill-learn" aria-label="Faux réveil"
        onClick={() => { void voiceIpc.falseWake(); setRecent(false); }}>Faux</button>}
    </>
  );
}

/** Annonce d'une mise à jour prête : hors de l'îlot (pas de bouton dans un bouton), sans prendre le focus. */
function UpdateToast() {
  const [version, setVersion] = useState<string | null>(null);
  const [waiting, setWaiting] = useState(false);
  useEffect(() => {
    const un = onUpdate((u) => setVersion(u.version));
    // Mise à jour déjà prête avant l'ouverture de la pilule (l'événement a pu partir sans elle).
    void updateIpc.status().then((s) => { if (s.pending) setVersion((v) => v ?? s.pending![0]); }).catch(() => {});
    return () => { void un.then((f) => f()); };
  }, []);
  if (!version) return null;
  return (
    <div className="pill-update" role="status">
      <span>Mise à jour {version} prête</span>
      {waiting ? <span>Installation dès que tu es disponible…</span> : <>
        <button type="button" className="pill-learn" onClick={() => { setWaiting(true); void updateIpc.restart().catch(() => setWaiting(false)); }}>Redémarrer</button>
        <button type="button" className="pill-learn" onClick={() => setVersion(null)}>Plus tard</button>
      </>}
    </div>
  );
}

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
        <PillToast />
        <VoiceOrb thinking={thinking} />
      </button>
      <UpdateToast />
      {settings.voice_wake && settings.voice_wake_learning && <LearnButtons />}
    </div>
  );
}
