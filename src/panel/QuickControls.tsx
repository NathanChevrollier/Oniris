import { useEffect, useMemo, useRef, useState, type CSSProperties, type RefObject } from "react";
import { Activity, Bluetooth, BluetoothOff, Lock, Moon, Power, PowerOff, RotateCcw, SlidersHorizontal, Volume2, VolumeX } from "lucide-react";
import { invoke } from "../lib/ipc";
import { usePanelStore } from "./store";
import { Mixer } from "./Mixer";
import "./quick.css";

/** Miroirs de `modules::pc::{Sound, PcState, AppRow}`, `modules::audio::Output` et `oniris_core::general::pc::Power`. */
export type Sound = { volume: { percent: number; muted: boolean } | null; volume_error?: string | null; output?: string | null };
export type PcState = Sound & { bluetooth: boolean | null };
export type AppRow = { key: string; name: string; percent: number; muted: boolean; active: boolean; icon: string | null };
export type Output = { id: string; name: string; percent: number; muted: boolean; default: boolean };
export type PowerAction = "lock" | "sleep" | "restart" | "shutdown";

export const pcIpc = {
  state: () => invoke<PcState>("pc_state"),
  sound: () => invoke<Sound>("pc_sound"),
  apps: () => invoke<AppRow[]>("pc_apps"),
  setApp: (key: string, percent: number | null, muted: boolean | null) => invoke<void>("pc_set_app", { key, percent, muted }),
  outputs: () => invoke<Output[]>("pc_outputs"),
  setOutput: (id: string, percent: number | null, muted: boolean | null) => invoke<void>("pc_set_output", { id, percent, muted }),
  setVolume: (percent: number) => invoke<void>("pc_set_volume", { percent }),
  setMute: (muted: boolean) => invoke<void>("pc_set_mute", { muted }),
  setBluetooth: (on: boolean) => invoke<void>("pc_set_bluetooth", { on }),
  taskManager: () => invoke<void>("pc_task_manager"),
  windowsSettings: () => invoke<void>("pc_windows_settings"),
  power: (action: PowerAction) => invoke<void>("pc_power", { action }),
};

/** Une écriture en vol au plus ; la dernière valeur demandée part à la fin de la précédente (curseur qu'on glisse). */
export function latest<T>(send: (v: T) => Promise<unknown>, onError: (e: unknown) => void) {
  let busy = false;
  let next: { v: T } | null = null;
  const push = (v: T): void => {
    if (busy) { next = { v }; return; }
    busy = true;
    send(v).catch(onError).finally(() => {
      busy = false;
      const n = next;
      next = null;
      if (n) push(n.v);
    });
  };
  return { push, busy: () => busy };
}

/** Molette sur un curseur : ±2 %, sans faire défiler le panneau (écouteur non passif, React les rend passifs). */
export function useWheelStep(ref: RefObject<HTMLElement | null>, step: (delta: number) => void) {
  const cb = useRef(step);
  cb.current = step;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if ((el as HTMLInputElement).disabled || e.deltaY === 0) return;
      e.preventDefault();
      cb.current(e.deltaY < 0 ? 2 : -2);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [ref]);
}

/** Relecture toutes les 3 s tant que `on` (le volume a pu changer ailleurs : clavier, Windows, casque). */
export function usePoll(on: boolean, load: () => void, ms = 3000) {
  const cb = useRef(load);
  cb.current = load;
  useEffect(() => {
    if (!on) return;
    cb.current();
    const t = setInterval(() => cb.current(), ms);
    return () => clearInterval(t);
  }, [on, ms]);
}

export const clamp = (n: number) => Math.max(0, Math.min(100, Math.round(n)));

const POWER: { action: PowerAction; label: string; confirm?: string; Icon: typeof Lock }[] = [
  { action: "lock", label: "Verrouiller", Icon: Lock },
  { action: "sleep", label: "Veille", confirm: "Confirmer la veille", Icon: Moon },
  { action: "restart", label: "Redémarrer", confirm: "Confirmer le redémarrage", Icon: RotateCcw },
  { action: "shutdown", label: "Éteindre", confirm: "Confirmer l'extinction", Icon: PowerOff },
];

function PowerMenu({ onError }: { onError: (e: unknown) => void }) {
  const [open, setOpen] = useState(false);
  const [armed, setArmed] = useState<PowerAction | null>(null);
  const wrap = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(null), 4000); // une confirmation oubliée ne reste pas armée
    return () => clearTimeout(t);
  }, [armed]);
  useEffect(() => {
    if (open) wrap.current?.querySelector<HTMLElement>(".qc-menu button")?.focus();
    else setArmed(null);
  }, [open]);
  const pick = (p: (typeof POWER)[number]) => {
    if (p.confirm && armed !== p.action) return setArmed(p.action);
    setOpen(false);
    pcIpc.power(p.action).catch(onError);
  };
  return (
    <div className="qc-power" ref={wrap} onBlur={(e) => { if (!wrap.current?.contains(e.relatedTarget as Node | null)) setOpen(false); }} onKeyDown={(e) => { if (e.key === "Escape" && open) { e.stopPropagation(); setOpen(false); wrap.current?.querySelector<HTMLElement>(".qc-btn")?.focus(); } }}>
      <button type="button" className="qc-btn" aria-label="Alimentation" title="Alimentation" aria-expanded={open} aria-haspopup="true" onClick={() => setOpen(!open)}><Power size={14} /></button>
      {open && (
        <div className="qc-menu" role="group" aria-label="Alimentation">
          {POWER.map((p) => (
            <button type="button" key={p.action} className={armed === p.action ? "armed" : undefined} onClick={() => pick(p)}>
              <p.Icon size={13} />{armed === p.action ? p.confirm : p.label}
            </button>
          ))}
          <span className="qc-sr" aria-live="polite">{armed ? "Clique encore pour confirmer." : ""}</span>
        </div>
      )}
    </div>
  );
}

/** Contrôles rapides façon barre des tâches : son, Bluetooth, Gestionnaire des tâches, Paramètres, alimentation. */
export function QuickControls() {
  const open = usePanelStore((s) => s.open);
  const [st, setSt] = useState<PcState | null>(null);
  const [err, setErr] = useState("");
  const touched = useRef(0);
  const slider = useRef<HTMLInputElement>(null);
  const writer = useMemo(() => latest((p: number) => pcIpc.setVolume(p), (e) => setErr(String(e))), []);

  useEffect(() => {
    if (open) pcIpc.state().then(setSt).catch(() => {});
  }, [open]);
  // Pas de relecture pendant un glissé ni juste après : le curseur ne saute pas sous la souris.
  usePoll(open, () => {
    if (writer.busy() || Date.now() - touched.current < 1500) return;
    pcIpc.sound().then((s) => setSt((o) => o && { ...o, ...s })).catch(() => {});
  });
  useEffect(() => {
    if (!err) return;
    const t = setTimeout(() => setErr(""), 5000);
    return () => clearTimeout(t);
  }, [err]);

  const fail = (e: unknown) => setErr(String(e));
  const volume = st?.volume;
  const bt = st?.bluetooth;
  const muted = !!volume?.muted;
  const setPercent = (p: number) => {
    if (!st || !volume) return;
    touched.current = Date.now();
    setSt({ ...st, volume: { percent: p, muted: false } });
    writer.push(p);
  };
  useWheelStep(slider, (d) => { if (volume) setPercent(clamp(volume.percent + d)); });
  const why = !st ? "Lecture du son…" : volume ? `Sortie : ${st.output ?? "par défaut"}` : `Son indisponible : ${st.volume_error ?? "aucune sortie audio"}`;
  return (
    <div className="qc" role="group" aria-label="Contrôles rapides">
      <div className="qc-row">
        <div className="qc-sound" title={why}>
          <button type="button" className="qc-btn" disabled={!volume} aria-pressed={muted} aria-label={muted ? "Rétablir le son" : "Couper le son"}
            onClick={() => { if (!volume) return; touched.current = Date.now(); setSt({ ...st!, volume: { ...volume, muted: !muted } }); pcIpc.setMute(!muted).catch(fail); }}>
            {muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
          </button>
          <input ref={slider} type="range" className="qc-vol" min={0} max={100} step={1} disabled={!volume} aria-label="Volume" aria-valuetext={`${volume?.percent ?? 0} %`}
            aria-description={why} value={volume?.percent ?? 0} style={{ "--v": `${volume?.percent ?? 0}%` } as CSSProperties}
            onChange={(e) => setPercent(Number(e.target.value))} />
          <span className="qc-pct" aria-hidden="true">{volume ? volume.percent : "–"}</span>
        </div>
        <Mixer open={open} onError={fail} />
        <button type="button" className={`qc-btn${bt ? " on" : ""}`} disabled={bt == null} aria-pressed={!!bt}
          aria-label={bt == null ? "Bluetooth indisponible" : bt ? "Désactiver le Bluetooth" : "Activer le Bluetooth"}
          title={bt == null ? "Aucun adaptateur Bluetooth" : bt ? "Bluetooth activé" : "Bluetooth désactivé"}
          onClick={() => {
            if (bt == null) return;
            setSt({ ...st!, bluetooth: !bt });
            pcIpc.setBluetooth(!bt).catch((e) => { setSt((s) => s && { ...s, bluetooth: bt }); fail(e); });
          }}>
          {bt === false ? <BluetoothOff size={14} /> : <Bluetooth size={14} />}
        </button>
        <button type="button" className="qc-btn" aria-label="Gestionnaire des tâches" title="Gestionnaire des tâches" onClick={() => pcIpc.taskManager().catch(fail)}><Activity size={14} /></button>
        <button type="button" className="qc-btn" aria-label="Paramètres Windows" title="Paramètres Windows" onClick={() => pcIpc.windowsSettings().catch(fail)}><SlidersHorizontal size={14} /></button>
        <PowerMenu onError={fail} />
      </div>
      <div className="qc-err" role="status">{err}</div>
    </div>
  );
}
