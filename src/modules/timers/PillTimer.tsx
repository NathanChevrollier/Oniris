import { useEffect, useState } from "react";
import { BellRing } from "lucide-react";
import { listen } from "../../lib/ipc";
import { useNow } from "../../lib/time";
import { alarmsIpc, mmss, timersIpc, type Rung, type Timer } from "./api";

/**
 * Compte à rebours de la pilule. La fenêtre de la pilule charge elle-même ses minuteurs (le drapeau « ouvert »
 * du panneau n'y a aucun sens) : au montage et à chaque `oniris://changed` du sujet « timers ».
 * Le tic d'une seconde n'existe que tant qu'un minuteur court : sinon aucune minuterie.
 * Une alarme qui sonne passe devant : un clic dessus l'arrête (sans ouvrir le panneau ni voler le focus).
 */
export function PillTimer() {
  const [list, setList] = useState<Timer[]>([]);
  const [ringing, setRinging] = useState<Rung[]>([]);
  useEffect(() => {
    let alive = true;
    const load = () => {
      timersIpc.list().then((l) => { if (alive) setList(l); }, () => {});
      alarmsIpc.state().then((s) => { if (alive) setRinging(s.ringing); }, () => {});
    };
    load();
    const un = listen<{ topic: string }>("oniris://changed", (e) => { if (e.payload.topic === "timers") load(); });
    return () => { alive = false; un.then((f) => f()); };
  }, []);

  const at = Date.now();
  const live = list.filter((t) => t.ends_ms > at); // le suivant prend le relais dès que le premier est fini
  useNow(1000, live.length > 0 && ringing.length === 0);
  if (ringing.length > 0) {
    const label = ringing.map((r) => r.label).join(", ");
    // Dans le bouton de la pilule : le clic s'arrête ici. Au clavier, la carte Minuteur a « Arrêter ».
    return (
      <span className="pill-timer pill-alarm" title={`${label} — clique pour arrêter`}
        onClick={(e) => { e.stopPropagation(); void alarmsIpc.stop().catch(() => {}); }}>
        <BellRing size={12} aria-hidden="true" /> {label}
      </span>
    );
  }
  if (live.length === 0) return null;
  return <span className="pill-timer" title={live[0].label}>{mmss(live[0].ends_ms - at)}</span>;
}
