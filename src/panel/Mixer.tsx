import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { AppWindow, AudioLines, Speaker, Volume2, VolumeX, type LucideIcon } from "lucide-react";
import { clamp, latest, pcIpc, usePoll, useWheelStep, type AppRow, type Output } from "./QuickControls";

type Row = { name: string; percent: number; muted: boolean; icon?: string | null; note: string; idle?: boolean };
type SetFn = (percent: number | null, muted: boolean | null) => Promise<void>;

/** Une ligne : icône, nom, curseur (glisser, clic, molette ±2 %) et muet. La relecture n'écrase pas un glissé en cours. */
function MixRow({ row, Icon, onSet, onError }: { row: Row; Icon: LucideIcon; onSet: SetFn; onError: (e: unknown) => void }) {
  const [v, setV] = useState({ percent: row.percent, muted: row.muted });
  const touched = useRef(0);
  const set = useRef(onSet);
  set.current = onSet;
  const fail = useRef(onError);
  fail.current = onError;
  const writer = useMemo(() => latest((p: number) => set.current(p, null), (e) => fail.current(e)), []);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!writer.busy() && Date.now() - touched.current > 1500) setV({ percent: row.percent, muted: row.muted });
  }, [row.percent, row.muted, writer]);
  const setPercent = (p: number) => {
    touched.current = Date.now();
    setV({ percent: p, muted: false });
    writer.push(p);
  };
  useWheelStep(input, (d) => setPercent(clamp(v.percent + d)));
  return (
    <li className={`mx-row${row.idle ? " idle" : ""}`} title={row.note}>
      {row.icon ? <img src={row.icon} alt="" width={16} height={16} /> : <Icon size={15} aria-hidden="true" />}
      <span className="mx-name">{row.name}</span>
      <input ref={input} type="range" className="qc-vol" min={0} max={100} step={1} aria-label={`Volume de ${row.name}`} aria-valuetext={`${v.percent} %`}
        value={v.percent} style={{ "--v": `${v.percent}%` } as CSSProperties} onChange={(e) => setPercent(Number(e.target.value))} />
      <button type="button" className="qc-btn" aria-pressed={v.muted} aria-label={v.muted ? `Rétablir ${row.name}` : `Couper ${row.name}`}
        onClick={() => { touched.current = Date.now(); setV({ ...v, muted: !v.muted }); set.current(null, !v.muted).catch(fail.current); }}>
        {v.muted ? <VolumeX size={13} /> : <Volume2 size={13} />}
      </button>
    </li>
  );
}

/** Mélangeur déroulant : volume par application (toutes sorties confondues) puis par sortie audio. */
export function Mixer({ open: panelOpen, onError }: { open: boolean; onError: (e: unknown) => void }) {
  const [open, setOpen] = useState(false);
  const [apps, setApps] = useState<AppRow[] | null>(null);
  const [outs, setOuts] = useState<Output[]>([]);
  const [err, setErr] = useState("");
  const wrap = useRef<HTMLDivElement>(null);
  const show = open && panelOpen;
  useEffect(() => { if (!panelOpen) setOpen(false); }, [panelOpen]);
  usePoll(show, () => {
    pcIpc.apps().then((a) => { setApps(a); setErr(""); }, (e) => setErr(String(e)));
    pcIpc.outputs().then(setOuts, () => {});
  });
  const outputs = [...outs].sort((a, b) => Number(b.default) - Number(a.default));
  return (
    <div className="qc-power" ref={wrap} onBlur={(e) => { if (!wrap.current?.contains(e.relatedTarget as Node | null)) setOpen(false); }}
      onKeyDown={(e) => { if (e.key === "Escape" && open) { e.stopPropagation(); setOpen(false); wrap.current?.querySelector<HTMLElement>(".qc-btn")?.focus(); } }}>
      <button type="button" className="qc-btn" aria-label="Mélangeur" title="Volume par application et par sortie" aria-expanded={show} aria-haspopup="true" onClick={() => setOpen(!open)}>
        <AudioLines size={14} />
      </button>
      {show && (
        <div className="qc-menu mx" role="group" aria-label="Mélangeur">
          <h4>Applications</h4>
          {err ? <p className="mx-empty">{err}</p>
            : !apps ? <p className="mx-empty">Lecture…</p>
            : apps.length === 0 ? <p className="mx-empty">Aucune application n'a de son ouvert.</p>
            : (
              <ul>
                {apps.map((a) => (
                  <MixRow key={a.key} Icon={AppWindow} onError={onError} onSet={(p, m) => pcIpc.setApp(a.key, p, m)}
                    row={{ name: a.name, percent: a.percent, muted: a.muted, icon: a.icon, idle: !a.active, note: a.active ? `${a.name} : joue du son` : `${a.name} : silencieux` }} />
                ))}
              </ul>
            )}
          {outputs.length > 0 && (
            <>
              <h4>Sorties</h4>
              <ul>
                {outputs.map((o) => (
                  <MixRow key={o.id} Icon={Speaker} onError={onError} onSet={(p, m) => pcIpc.setOutput(o.id, p, m)}
                    row={{ name: o.name, percent: o.percent, muted: o.muted, note: o.default ? `${o.name} (sortie par défaut, celle du curseur)` : o.name }} />
                ))}
              </ul>
            </>
          )}
        </div>
      )}
    </div>
  );
}
