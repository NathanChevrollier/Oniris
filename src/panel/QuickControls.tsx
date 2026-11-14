import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Activity, Bluetooth, BluetoothOff, Lock, Moon, Power, PowerOff, RotateCcw, SlidersHorizontal, Volume2, VolumeX } from "lucide-react";
import { invoke } from "../lib/ipc";
import { usePanelStore } from "./store";
import "./quick.css";

/** Miroirs de `modules::pc::PcState` et `oniris_core::general::pc::Power`. */
export type PcState = { volume: { percent: number; muted: boolean } | null; bluetooth: boolean | null };
export type PowerAction = "lock" | "sleep" | "restart" | "shutdown";

export const pcIpc = {
  state: () => invoke<PcState>("pc_state"),
  setVolume: (percent: number) => invoke<void>("pc_set_volume", { percent }),
  setMute: (muted: boolean) => invoke<void>("pc_set_mute", { muted }),
  setBluetooth: (on: boolean) => invoke<void>("pc_set_bluetooth", { on }),
  taskManager: () => invoke<void>("pc_task_manager"),
  windowsSettings: () => invoke<void>("pc_windows_settings"),
  power: (action: PowerAction) => invoke<void>("pc_power", { action }),
};

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
  // Une seule écriture du volume en vol ; la dernière valeur du curseur part à la fin de la précédente.
  const vol = useRef<{ busy: boolean; next: number | null }>({ busy: false, next: null });

  useEffect(() => {
    if (open) pcIpc.state().then(setSt).catch(() => {});
  }, [open]);
  useEffect(() => {
    if (!err) return;
    const t = setTimeout(() => setErr(""), 5000);
    return () => clearTimeout(t);
  }, [err]);

  const fail = (e: unknown) => setErr(String(e));
  const pushVolume = (percent: number) => {
    const v = vol.current;
    if (v.busy) { v.next = percent; return; }
    v.busy = true;
    pcIpc.setVolume(percent).catch(fail).finally(() => {
      v.busy = false;
      const n = v.next;
      v.next = null;
      if (n !== null && n !== percent) pushVolume(n);
    });
  };

  const volume = st?.volume;
  const bt = st?.bluetooth;
  const muted = !!volume?.muted;
  return (
    <div className="qc" role="group" aria-label="Contrôles rapides">
      <div className="qc-row">
        <button type="button" className="qc-btn" disabled={!volume} aria-pressed={muted} aria-label={muted ? "Rétablir le son" : "Couper le son"} title={muted ? "Rétablir le son" : "Couper le son"}
          onClick={() => { if (!volume) return; setSt({ ...st!, volume: { ...volume, muted: !muted } }); pcIpc.setMute(!muted).catch(fail); }}>
          {muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
        </button>
        <input type="range" className="qc-vol" min={0} max={100} step={1} disabled={!volume} aria-label="Volume" aria-valuetext={`${volume?.percent ?? 0} %`}
          value={volume?.percent ?? 0} style={{ "--v": `${volume?.percent ?? 0}%` } as CSSProperties}
          onChange={(e) => { const p = Number(e.target.value); if (!volume) return; setSt({ ...st!, volume: { percent: p, muted: false } }); pushVolume(p); }} />
        <span className="qc-pct" aria-hidden="true">{volume ? volume.percent : "–"}</span>
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
