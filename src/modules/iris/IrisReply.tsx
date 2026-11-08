import { useEffect } from "react";
import { Sparkles, X } from "lucide-react";
import { Orb } from "../../ui/Orb";
import { usePanelStore } from "../../panel/store";
import { onIris, type Brain } from "./api";
import { orbFor, useVoice } from "../voice/api";
import { Briefing, SegmentText } from "./Briefing";
import { apply, ask, dismiss, loadStatus, useIris } from "./store";
import "./iris.css";

/** Retour au briefing après cette durée sans échange en cours. */
export const IDLE_MS = 2 * 60_000;

const BRAIN: Record<Brain, string> = { fast: "éclair", local: "local", claude: "Claude" };

/** Sphère : écoute, réfléchit ou parle selon la voix et le tour ; repos sinon. Seul l'état est suivi, jamais le flux. */
export function IrisOrb({ size }: { size: number }) {
  const thinking = useIris((s) => s.state === "thinking");
  const voice = useVoice();
  return <Orb state={orbFor(voice.phase, thinking)} size={size} level={voice.level} />;
}

/** « Prêt · local », « Réfléchit · Claude » ou « Local indisponible » ; l'état de Rust est relu à l'ouverture seulement. */
export function IrisStatusLine() {
  const open = usePanelStore((s) => s.open);
  const thinking = useIris((s) => s.state === "thinking");
  const brain = useIris((s) => s.brain);
  const local = useIris((s) => s.status?.local);
  useEffect(() => { if (open) void loadStatus(); }, [open]);
  const down = !thinking && local != null && local !== "ready";
  const text = thinking ? `Réfléchit · ${brain ? BRAIN[brain] : "local"}` : down ? "Local indisponible" : local ? "Prêt · local" : "Prêt";
  return <div className={down ? "orb-status off" : "orb-status"} role="status"><i />{text}</div>;
}

/** Échange avec IRIS ; le briefing reprend la place sans échange (au bout de 2 min d'inactivité ou sur « × »). */
export function IrisReply() {
  const user = useIris((s) => s.user);
  const state = useIris((s) => s.state);
  const segments = useIris((s) => s.segments);
  const brain = useIris((s) => s.brain);
  const tools = useIris((s) => s.toolsUsed);
  const pending = useIris((s) => s.pending.length);
  const error = useIris((s) => s.error);

  useEffect(() => {
    const un = onIris(apply);
    return () => { un.then((f) => f()); };
  }, []);
  useEffect(() => {
    if (!user || state === "thinking") return;
    const t = setTimeout(dismiss, IDLE_MS);
    return () => clearTimeout(t);
  }, [user, state]);

  if (!user) return <Briefing />;
  const thinking = state === "thinking";
  return (
    <div className="brief iris-reply" aria-busy={thinking}>
      <div className="spark"><Sparkles size={15} /></div>
      <div className="iris-body">
        <p className="iris-user">{user}</p>
        <p className="iris-text" aria-live="polite">{segments.length ? segments.map((s, i) => <SegmentText key={i} s={s} />) : thinking ? "…" : null}</p>
        {error && (
          <div className="iris-err" role="alert">
            <p>{error.message}</p>
            <div className="iris-chips">
              <button type="button" className="iris-chip" onClick={dismiss}>Réessayer plus tard</button>
              {error.offer_local && <button type="button" className="iris-chip" onClick={() => ask(user, false, true)}>Version simple en local</button>}
            </div>
          </div>
        )}
        <div className="iris-meta">
          {brain && <span className="iris-brain" data-brain={brain}>{BRAIN[brain]}</span>}
          {tools.map((t) => <span key={t} className="iris-tool">{t}</span>)}
          {pending > 0 && <span className="iris-tool">À confirmer</span>}
        </div>
      </div>
      {!thinking && <button type="button" className="iris-close" aria-label="Fermer la réponse" onClick={dismiss}><X size={14} /></button>}
    </div>
  );
}
