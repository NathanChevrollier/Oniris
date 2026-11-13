import { createContext, useCallback, useContext, useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS, type Settings } from "../../lib/settings";
import { errorText } from "../accounts/api";

type Save = (patch: Partial<Settings>) => Promise<boolean>;
type KeysOf<T> = { [K in keyof Settings]: Settings[K] extends T ? K : never }[keyof Settings];

export const SetCtx = createContext<{ s: Settings; save: Save }>({ s: DEFAULT_SETTINGS, save: async () => false });
export const useSet = () => useContext(SetCtx);

/** Enregistrement auto : réglages relus juste avant d'écrire, écritures en file pour qu'un clic rapide n'écrase pas le précédent. */
export function useSaver() {
  const [msg, setMsg] = useState<{ ok: boolean; text: string; n: number } | null>(null);
  const chain = useRef(Promise.resolve(true));
  const save = useCallback<Save>((patch) => (chain.current = chain.current.then(async () => {
    try {
      await ipc.saveSettings({ ...(await ipc.getSettings()), ...patch });
      setMsg((m) => ({ ok: true, text: "Enregistré", n: (m?.n ?? 0) + 1 }));
      return true;
    } catch (e) { setMsg((m) => ({ ok: false, text: errorText(e, "Enregistrement impossible."), n: (m?.n ?? 0) + 1 })); return false; }
  })), []);
  useEffect(() => { if (!msg?.ok) return; const t = setTimeout(() => setMsg(null), 2200); return () => clearTimeout(t); }, [msg]);
  return [save, msg, () => setMsg(null)] as const;
}

/** Brouillon local, remis à la valeur enregistrée seulement quand celle-ci change. */
export function useDraft<T>(v: T) {
  const key = JSON.stringify(v);
  const [d, setD] = useState(v);
  const [k, setK] = useState(key);
  if (k !== key) { setK(key); setD(v); }
  return [d, setD] as const;
}

export function Group({ title, note, children }: { title?: string; note?: ReactNode; children: ReactNode }) {
  return (
    <section className="set-group">
      {title && <h4>{title}</h4>}
      <div className="set-card">{children}</div>
      {note && <p className="set-foot">{note}</p>}
    </section>
  );
}

/** Ligne : titre et description à gauche, contrôle à droite. `id` relie le titre au contrôle. */
export function Row({ label, desc, id, stack, children }: { label: ReactNode; desc?: ReactNode; id?: string; stack?: boolean; children?: ReactNode }) {
  return (
    <div className={stack ? "set-row stack" : "set-row"}>
      <div className="set-txt">
        {id ? <label htmlFor={id} className="set-label">{label}</label> : <span className="set-label">{label}</span>}
        {desc && <p className="set-desc" id={id && `${id}-d`}>{desc}</p>}
      </div>
      {children && <div className="set-ctl">{children}</div>}
    </div>
  );
}

const described = (id: string, desc: unknown) => (desc ? `${id}-d` : undefined);
const patchOf = <K extends keyof Settings>(k: K, v: Settings[K]) => ({ [k]: v }) as Partial<Settings>;

/** Interrupteur : vrai `<input type="checkbox">`, enregistré au clic ; revient en arrière si Rust refuse. */
export function Toggle({ k, label, desc, disabled, children }: { k: KeysOf<boolean>; label: string; desc?: ReactNode; disabled?: boolean; children?: ReactNode }) {
  const { s, save } = useSet();
  const id = useId();
  const [on, setOn] = useDraft(s[k]);
  return (
    <Row label={label} desc={desc} id={id}>
      {children}
      <input id={id} type="checkbox" className="set-switch" checked={on} disabled={disabled} aria-describedby={described(id, desc)}
        onChange={(e) => { const v = e.target.checked; setOn(v); void save(patchOf(k, v)).then((ok) => { if (!ok) setOn(!v); }); }} />
    </Row>
  );
}

/** Champ texte ou nombre, enregistré à la perte du focus ou sur Entrée. La valeur part telle quelle : Rust seul la refuse. */
export function Field({ k, label, desc, unit, placeholder, min, max, wide }: {
  k: KeysOf<string> | KeysOf<number>; label: string; desc?: ReactNode; unit?: string; placeholder?: string; min?: number; max?: number; wide?: boolean;
}) {
  const { s, save } = useSet();
  const id = useId();
  const v = String(s[k]);
  const [d, setD] = useDraft(v);
  const num = typeof s[k] === "number";
  const commit = () => { if (d !== v) void save({ [k]: num ? Number(d) : d.trim() }); };
  const input = (
    <input id={id} type={num ? "number" : "text"} className={num ? undefined : wide ? "set-input wide" : "set-input"} value={d} min={min} max={max}
      placeholder={placeholder} spellCheck={false} aria-describedby={described(id, desc)}
      onChange={(e) => setD(e.target.value)} onBlur={commit} onKeyDown={(e) => { if (e.key === "Enter") commit(); }} />
  );
  return <Row label={label} desc={desc} id={id}>{num ? <span className="set-num">{input}{unit && <i>{unit}</i>}</span> : input}</Row>;
}

/** Texte multi-lignes enregistré à la perte du focus ; `list` : une entrée par ligne. */
export function Area({ k, label, desc, rows = 3, list, max, after }: {
  k: KeysOf<string> | KeysOf<string[]>; label: string; desc?: ReactNode; rows?: number; list?: boolean; max?: number; after?: (d: string, set: (v: string) => void) => ReactNode;
}) {
  const { s, save } = useSet();
  const id = useId();
  const raw = s[k];
  const v = Array.isArray(raw) ? raw.join("\n") : raw;
  const [d, setD] = useDraft(v);
  const commit = () => { if (d !== v) void save({ [k]: list ? d.split("\n").map((l) => l.trim()).filter(Boolean) : d }); };
  return (
    <Row label={label} desc={desc} id={id} stack>
      <textarea id={id} className="set-area" rows={rows} maxLength={max} value={d} spellCheck={false} aria-describedby={described(id, desc)}
        onChange={(e) => setD(e.target.value)} onBlur={commit} />
      {after?.(d, setD)}
    </Row>
  );
}

/** Liste déroulante enregistrée au choix ; une valeur inconnue (réglée ailleurs) reste affichée. */
export function Choice({ k, label, desc, options, disabled }: { k: KeysOf<string>; label: string; desc?: ReactNode; options: [string, string][]; disabled?: boolean }) {
  const { s, save } = useSet();
  const id = useId();
  const [d, setD] = useDraft(s[k]);
  const all = options.some(([o]) => o === d) ? options : [...options, [d, d] as [string, string]];
  return (
    <Row label={label} desc={desc} id={id}>
      <select id={id} className="set-select" value={d} disabled={disabled} aria-describedby={described(id, desc)}
        onChange={(e) => { const v = e.target.value; setD(v); void save(patchOf(k, v)).then((ok) => { if (!ok) setD(s[k]); }); }}>
        {all.map(([o, l]) => <option key={o} value={o}>{l}</option>)}
      </select>
    </Row>
  );
}

/** Contrôle segmenté (boutons radio) pour deux ou trois choix. */
export function Segmented({ k, label, desc, options }: { k: KeysOf<string>; label: string; desc?: ReactNode; options: [string, string][] }) {
  const { s, save } = useSet();
  const id = useId();
  const [d, setD] = useDraft(s[k]);
  return (
    <Row label={label} desc={desc}>
      <div className="set-seg" role="radiogroup" aria-label={label}>
        {options.map(([o, l]) => (
          <label key={o}><input type="radio" name={id} value={o} checked={d === o}
            onChange={() => { setD(o); void save(patchOf(k, o as never)).then((ok) => { if (!ok) setD(s[k]); }); }} /><span>{l}</span></label>
        ))}
      </div>
    </Row>
  );
}

/** Curseur enregistré peu après le dernier mouvement. */
export function Slider({ k, label, desc, min = 0, max = 100, unit = "" }: { k: KeysOf<number>; label: string; desc?: ReactNode; min?: number; max?: number; unit?: string }) {
  const { s, save } = useSet();
  const id = useId();
  const [d, setD] = useDraft(s[k]);
  useEffect(() => {
    if (d === s[k]) return;
    const t = setTimeout(() => void save(patchOf(k, d)), 350);
    return () => clearTimeout(t);
  }, [d, k, s, save]);
  return (
    <Row label={label} desc={desc} id={id}>
      <input id={id} type="range" className="set-range" min={min} max={max} value={d} aria-describedby={described(id, desc)}
        style={{ "--v": `${((d - min) / (max - min)) * 100}%` } as CSSProperties} onChange={(e) => setD(Number(e.target.value))} />
      <output htmlFor={id} className="set-val">{d}{unit}</output>
    </Row>
  );
}
