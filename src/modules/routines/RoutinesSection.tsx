import { useCallback, useEffect, useState } from "react";
import { ArrowDown, ArrowUp, Pencil, Play, Plus, Trash2, X } from "lucide-react";
import { errorText } from "../accounts/api";
import { favoritesIpc, type Favorite } from "../favorites/api";
import { Group } from "../settings/controls";
import { routinesIpc, type Routine, type Step } from "./api";
import { defaultArgs, missingField, TOOLS, type ArgField } from "./tools";
import "./routines.css";

// Catégorie « Routines » des Réglages : liste, création, édition, lancement et suppression.

const MAX_ROUTINES = 30;
const MAX_STEPS = 20;
const blank = (): Routine => ({ name: "", phrases: [], steps: [{ tool: "apps.launch", args: {}, blocking: false }] });

function ArgInput({ f, value, onChange, favorites }: { f: ArgField; value: unknown; onChange: (v: unknown) => void; favorites: Favorite[] }) {
  const label = `${f.title}${f.required ? "" : " (facultatif)"}`;
  if (f.kind === "bool") {
    return <label className="rt-arg"><input type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked ? true : f.required ? false : undefined)} /> {f.title}</label>;
  }
  if (f.kind === "choice" || f.kind === "favorite") {
    const opts: [string, string][] = f.kind === "choice" ? f.choices ?? [] : favorites.map((x) => [String(x.id), x.title]);
    return (
      <label className="rt-arg">{label}
        <select className="set-select" value={value === undefined ? "" : String(value)}
          onChange={(e) => onChange(f.kind === "favorite" ? (e.target.value ? Number(e.target.value) : undefined) : e.target.value)}>
          {f.kind === "favorite" && <option value="">Choisir…</option>}
          {opts.map(([o, l]) => <option key={o} value={o}>{l}</option>)}
        </select>
      </label>
    );
  }
  return (
    <label className="rt-arg">{label}
      <input className="set-input" type={f.kind === "number" ? "number" : "text"} min={f.min} max={f.max} placeholder={f.placeholder} spellCheck={false}
        value={value === undefined ? "" : String(value)}
        onChange={(e) => { const v = e.target.value; onChange(v === "" ? undefined : f.kind === "number" ? Number(v) : v); }} />
    </label>
  );
}

function Editor({ initial, onSave, onCancel }: { initial: Routine; onSave: (r: Routine) => Promise<string | null>; onCancel: () => void }) {
  const [r, setR] = useState(initial);
  const [phrases, setPhrases] = useState(initial.phrases.join("\n"));
  const [err, setErr] = useState<string | null>(null);
  const [favorites, setFavorites] = useState<Favorite[]>([]);
  useEffect(() => { favoritesIpc.list().then(setFavorites, () => {}); }, []);
  const setStep = (i: number, s: Step) => setR({ ...r, steps: r.steps.map((x, j) => (j === i ? s : x)) });
  const move = (i: number, d: number) => {
    const steps = [...r.steps];
    [steps[i], steps[i + d]] = [steps[i + d], steps[i]];
    setR({ ...r, steps });
  };
  const submit = async () => {
    if (!r.name.trim()) return setErr("Donne un nom à la routine.");
    for (const [i, s] of r.steps.entries()) {
      const m = missingField(s.tool, s.args);
      if (m) return setErr(`Étape ${i + 1} : « ${m} » est obligatoire.`);
    }
    const args = (a: Record<string, unknown>) => Object.fromEntries(Object.entries(a).filter(([, v]) => v !== undefined));
    setErr(await onSave({ ...r, phrases: phrases.split("\n").map((p) => p.trim()).filter(Boolean), steps: r.steps.map((s) => ({ ...s, args: args(s.args) })) }));
  };
  return (
    <div className="set-row stack rt-editor" role="group" aria-label={initial.name ? `Modifier ${initial.name}` : "Nouvelle routine"}>
      <label className="rt-arg">Nom<input className="set-input" maxLength={60} value={r.name} placeholder="Mode jeu" onChange={(e) => setR({ ...r, name: e.target.value })} /></label>
      <label className="rt-arg">Phrases de déclenchement (facultatif, une par ligne)
        <textarea className="set-area" rows={2} value={phrases} placeholder={"on joue\nc'est l'heure de jouer"} onChange={(e) => setPhrases(e.target.value)} />
      </label>
      <ol className="rt-steps">
        {r.steps.map((s, i) => (
          <li key={i} className="rt-step">
            <div className="rt-step-head">
              <select className="set-select" aria-label={`Action de l'étape ${i + 1}`} value={s.tool}
                onChange={(e) => setStep(i, { ...s, tool: e.target.value, args: defaultArgs(e.target.value) })}>
                {Object.entries(TOOLS).map(([t, d]) => <option key={t} value={t}>{d.label}</option>)}
              </select>
              <button type="button" className="acc-icon" aria-label={`Monter l'étape ${i + 1}`} disabled={i === 0} onClick={() => move(i, -1)}><ArrowUp size={14} /></button>
              <button type="button" className="acc-icon" aria-label={`Descendre l'étape ${i + 1}`} disabled={i === r.steps.length - 1} onClick={() => move(i, 1)}><ArrowDown size={14} /></button>
              <button type="button" className="acc-icon" aria-label={`Retirer l'étape ${i + 1}`} disabled={r.steps.length === 1} onClick={() => setR({ ...r, steps: r.steps.filter((_, j) => j !== i) })}><X size={14} /></button>
            </div>
            {TOOLS[s.tool]?.fields.map((f) => (
              <ArgInput key={f.key} f={f} value={s.args[f.key]} favorites={favorites} onChange={(v) => setStep(i, { ...s, args: { ...s.args, [f.key]: v } })} />
            ))}
            <label className="rt-arg"><input type="checkbox" checked={s.blocking} onChange={(e) => setStep(i, { ...s, blocking: e.target.checked })} /> Bloquante : si elle échoue, la suite n'est pas lancée</label>
            {TOOLS[s.tool]?.confirm && <p className="set-desc">Cette étape te demandera confirmation dans le panneau.</p>}
          </li>
        ))}
      </ol>
      <div className="set-bar">
        <button type="button" className="acc-btn" disabled={r.steps.length >= MAX_STEPS} onClick={() => setR({ ...r, steps: [...r.steps, { tool: "apps.launch", args: {}, blocking: false }] })}><Plus size={13} />Ajouter une étape</button>
        <span />
        <button type="button" className="acc-btn" onClick={onCancel}>Annuler</button>
        <button type="button" className="acc-btn primary" onClick={() => void submit()}>Enregistrer</button>
      </div>
      {err && <p className="err" role="alert">{err}</p>}
    </div>
  );
}

export function RoutinesSection() {
  const [list, setList] = useState<Routine[] | null>(null);
  const [editing, setEditing] = useState<number | "new" | null>(null);
  const [confirm, setConfirm] = useState<number | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const load = useCallback(() => { routinesIpc.list().then(setList, (e) => setMsg({ ok: false, text: errorText(e, "Impossible de charger les routines.") })); }, []);
  useEffect(load, [load]);
  const store = async (next: Routine[]): Promise<string | null> => {
    try { setList(await routinesIpc.save(next)); setEditing(null); return null; } catch (e) { return errorText(e, "Enregistrement impossible."); }
  };
  const run = (name: string) => {
    setMsg(null);
    routinesIpc.run(name).then((r) => setMsg({ ok: true, text: r.summary ?? `Routine « ${name} » lancée.` }), (e) => setMsg({ ok: false, text: errorText(e, "Lancement impossible.") }));
  };
  const remove = (i: number) => {
    if (!list) return;
    void store(list.filter((_, j) => j !== i)).then((e) => { if (e) setMsg({ ok: false, text: e }); setConfirm(null); });
  };
  const items = list ?? [];
  return (
    <Group title="Mes routines" note="Dis « lance le mode jeu » à IRIS, cherche « Routine » dans la barre de commande ou clique sur Lancer. Chaque étape passe par les mêmes règles qu'une demande directe : celles qui demandent confirmation affichent leur carte.">
      {msg && <div className="set-row"><p className={msg.ok ? "set-desc" : "err"} role={msg.ok ? "status" : "alert"}>{msg.text}</p></div>}
      {list && items.length === 0 && editing === null && <div className="set-row"><p className="set-desc">Aucune routine pour l'instant.</p></div>}
      <ul className="set-list">
        {items.map((r, i) => editing === i
          ? <li key={r.name}><Editor initial={r} onCancel={() => setEditing(null)} onSave={(x) => store(items.map((y, j) => (j === i ? x : y)))} /></li>
          : (
            <li key={r.name}>
              <span><b>{r.name}</b> <small className="set-desc">{r.steps.length} étape{r.steps.length > 1 ? "s" : ""}{r.phrases.length ? ` · « ${r.phrases.join(" », « ")} »` : ""}</small></span>
              {confirm === i ? (
                <span className="set-ctl" role="alertdialog" aria-label={`Supprimer ${r.name}`}>
                  <button type="button" className="acc-btn" onClick={() => setConfirm(null)}>Garder</button>
                  <button type="button" className="acc-btn primary" onClick={() => remove(i)}>Confirmer la suppression</button>
                </span>
              ) : (
                <span className="set-ctl">
                  <button type="button" className="acc-btn" onClick={() => run(r.name)}><Play size={13} />Lancer</button>
                  <button type="button" className="acc-icon" aria-label={`Modifier ${r.name}`} onClick={() => setEditing(i)}><Pencil size={14} /></button>
                  <button type="button" className="acc-icon" aria-label={`Supprimer ${r.name}`} onClick={() => setConfirm(i)}><Trash2 size={14} /></button>
                </span>
              )}
            </li>
          ))}
      </ul>
      {editing === "new"
        ? <Editor initial={blank()} onCancel={() => setEditing(null)} onSave={(x) => store([...items, x])} />
        : <div className="set-row"><button type="button" className="acc-btn" disabled={!list || items.length >= MAX_ROUTINES || editing !== null} onClick={() => setEditing("new")}><Plus size={13} />Nouvelle routine</button></div>}
    </Group>
  );
}
