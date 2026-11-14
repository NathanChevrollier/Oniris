import { useEffect, useRef, useState, type ReactNode } from "react";
import { ipc, onPanelState } from "../lib/ipc";
import { useSettings } from "../lib/settings";
import { hhmm, useNow } from "../lib/time";
import { PillTimer } from "../modules/timers/PillTimer";
import { Orb } from "../ui/Orb";
import { onIris } from "../modules/iris/api";
import { onVoice, orbFor, useVoice, voiceIpc } from "../modules/voice/api";
import { onUpdate, restartText, retry, updateIpc, type UpdateStatus } from "../modules/settings/update";
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

/** Veille : délai avant de se cacher quand le curseur s'en va, puis le temps que l'îlot remonte avant de réduire la fenêtre. */
const SLEEP_AFTER_MS = 800;
const SLEEP_SHRINK_MS = 300;

/** Le micro écoute ou IRIS parle. Abonné seulement en veille : la pilule ne se redessine pas au rythme du niveau. */
function useVoiceBusy(enabled: boolean) {
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!enabled) return setBusy(false);
    const un = onVoice((e) => setBusy(e.phase !== "idle"));
    return () => { void un.then((f) => f()); };
  }, [enabled]);
  return busy;
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

/** Mise à jour (téléchargement, prête, installation, échec), sinon l'annonce de la version installée.
 *  Hors de l'îlot (pas de bouton dans un bouton), sans prendre le focus. */
function UpdateToast() {
  const [st, setSt] = useState<UpdateStatus | null>(null);
  const [hidden, setHidden] = useState<string | null>(null);
  useEffect(() => {
    const un = onUpdate(setSt);
    // État déjà connu avant l'ouverture de la pilule (l'événement a pu partir sans elle).
    void updateIpc.status().then((s) => setSt((v) => v ?? s)).catch(() => {});
    return () => { void un.then((f) => f()); };
  }, []);
  if (!st) return null;
  const v = "version" in st ? st.version : null;
  const btn = (label: string, on: () => void) => <button type="button" className="pill-learn" onClick={on}>{label}</button>;
  const fail = (e: unknown) => setSt((s) => s && ({ ...s, state: "error", message: `Échec : ${String(e)}`, version: v }) as UpdateStatus);
  const later = (key: string) => btn("Plus tard", () => setHidden(key));
  let key = `${st.state}:${v}`;
  let body: ReactNode;
  if (st.state === "downloading") body = <span>Téléchargement de la mise à jour {v}…{st.percent !== null && ` ${st.percent} %`}</span>;
  else if (st.state === "ready") body = <><span>Mise à jour {v} prête</span>{btn("Redémarrer", () => void updateIpc.restart().catch(fail))}{later(key)}</>;
  else if (st.state === "waiting" || st.state === "installing") body = <span>{restartText(st)}</span>;
  else if (st.state === "error" && v) body = <><span>{st.message}</span>{btn("Réessayer", () => void retry(st).catch(fail))}{later(key)}</>;
  else if (st.updated) {
    key = `updated:${st.updated.version}`;
    body = <><span>Oniris est passé en {st.updated.version}</span>{btn("Nouveautés", () => void updateIpc.whatsNew().catch(() => {}))}{btn("Fermer", () => setHidden(key))}</>;
  }
  if (!body || hidden === key) return null;
  return <div className="pill-update" role="status">{body}</div>;
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
  const voiceBusy = useVoiceBusy(settings.pill_sleep);
  const [near, setNear] = useState(false);
  const leave = useRef<ReturnType<typeof setTimeout>>(undefined);
  const width = useRef(0);
  const shrunk = useRef(false);
  const asleep = settings.pill_sleep && !near && !panelOpen && !voiceBusy;

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
      t = setTimeout(() => { if (w !== last) { last = width.current = w; ipc.pillResize(w, shrunk.current); } }, 100);
    });
    ro.observe(root.current!);
    return () => { ro.disconnect(); clearTimeout(t); clearTimeout(leave.current); };
  }, []);

  // Veille : la fenêtre devient une bande de 2 px en haut de l'écran ; le curseur qui la touche la réveille.
  useEffect(() => {
    if (asleep === shrunk.current) return;
    shrunk.current = asleep;
    const send = () => void ipc.pillResize(width.current || 420, asleep);
    if (!asleep) { send(); return; }
    const t = setTimeout(send, SLEEP_SHRINK_MS);
    return () => clearTimeout(t);
  }, [asleep]);

  return (
    <div ref={root} className={`pill-root${panelOpen ? " open" : ""}${asleep ? " asleep" : ""}`}
      onMouseEnter={() => { clearTimeout(leave.current); setNear(true); }}
      onMouseLeave={() => { clearTimeout(leave.current); leave.current = setTimeout(() => setNear(false), SLEEP_AFTER_MS); }}>
      <button type="button" className="island" aria-label="Ouvrir le panneau Oniris"
        onClick={() => { cancelHover(); ipc.panelToggle(); }}
        onMouseEnter={() => {
          if (settings.hover_open && Date.now() - closedAt.current >= HOVER_REARM_MS) hover.current = setTimeout(() => ipc.panelOpen(), settings.hover_delay_ms);
        }}
        onMouseLeave={cancelHover}>
        <span className="time">{hhmm(now)}</span>
        <PillTimer />
        <PillToast />
        <VoiceOrb thinking={thinking} />
      </button>
      <UpdateToast />
      {settings.voice_wake && settings.voice_wake_learning && <LearnButtons />}
    </div>
  );
}
