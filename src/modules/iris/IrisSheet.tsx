import { useCallback, useEffect, useRef, useState } from "react";
import { ExternalLink, Trash2, X } from "lucide-react";
import { ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS, useSettings, type Settings } from "../../lib/settings";
import { accountsIpc, errorText } from "../accounts/api";
import { irisIpc, onIrisPull, type HistoryItem, type IrisStatus, type Memory } from "./api";
import "../accounts/accounts.css";
import "./iris.css";

const MAX_PERSONALITY = 2000;
const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);

/** Relit les réglages juste avant d'écrire (comme /météo) ; le texte de refus de Rust s'affiche tel quel. */
function useSave() {
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const save = async (patch: Partial<Settings>) => {
    try {
      const fresh = await ipc.getSettings();
      await ipc.saveSettings({ ...fresh, ...patch });
      setMsg({ ok: true, text: "Enregistré." });
    } catch (e) { setMsg({ ok: false, text: errorText(e, "Enregistrement impossible.") }); }
  };
  const note = msg && <p className={msg.ok ? "acc-ok" : "err"} role={msg.ok ? "status" : "alert"}>{msg.text}</p>;
  return [save, note] as const;
}

function Brains({ status, reload }: { status: IrisStatus | null; reload: () => void }) {
  const s = useSettings();
  const [model, setModel] = useState(s.iris_local_model);
  const [minutes, setMinutes] = useState(String(s.iris_unload_minutes));
  const [claudePath, setClaudePath] = useState(s.claude_path);
  const [claudeModel, setClaudeModel] = useState(s.iris_claude_model);
  const [pull, setPull] = useState<{ done: number; total: number } | null>(null);
  const [pullError, setPullError] = useState<string | null>(null);
  const [save, note] = useSave();
  useEffect(() => { setModel(s.iris_local_model); setMinutes(String(s.iris_unload_minutes)); setClaudePath(s.claude_path); setClaudeModel(s.iris_claude_model); }, [s]);
  useEffect(() => {
    const un = onIrisPull((e) => { if (e.error) setPullError(e.error); else setPull({ done: e.done ?? 0, total: e.total ?? 0 }); });
    return () => { void un.then((f) => f()); };
  }, []);
  const download = () => {
    setPullError(null); setPull({ done: 0, total: 0 });
    irisIpc.pullModel().catch((e) => setPullError(errorText(e, "Téléchargement impossible."))).finally(() => { setPull(null); reload(); });
  };
  const guide = () => accountsIpc.openGuide("https://ollama.com/download").catch((e) => setPullError(errorText(e, "Ouverture impossible.")));
  const pct = pull && pull.total > 0 ? Math.round((pull.done / pull.total) * 100) : null;
  return (
    <>
      <h3>Cerveaux</h3>
      <div className="iris-row">
        <b>Ollama (local)</b>
        {status?.local === "ready" && <span className="acc-state">Prêt · {status.model}</span>}
        {status?.local === "down" && <><span className="acc-state warn">Ollama introuvable</span>
          <button type="button" className="acc-btn primary" onClick={() => void guide()}><ExternalLink size={13} />Installer Ollama</button></>}
        {status?.local === "no_model" && <><span className="acc-state warn">Modèle absent</span>
          <button type="button" className="acc-btn primary" disabled={pull !== null} onClick={download}>Télécharger le modèle</button></>}
      </div>
      {pull && <progress className="iris-progress" aria-label="Téléchargement du modèle" max={pull.total || undefined} value={pull.total ? pull.done : undefined} />}
      {pull && <span role="status" className="acc-wait">Téléchargement{pct !== null ? ` : ${pct} %` : "…"}</span>}
      {pullError && <p className="err" role="alert">{pullError}</p>}
      <div className="iris-row">
        <b>Claude Code</b>
        {status?.claude.kind === "found"
          ? <span className="acc-state" title={status.claude.path}>Trouvé</span>
          : <span className="acc-state warn">Introuvable</span>}
      </div>
      <form className="iris-grid" onSubmit={(e) => { e.preventDefault(); void save({ iris_local_model: model, iris_unload_minutes: Number(minutes), claude_path: claudePath, iris_claude_model: claudeModel }).then(reload); }}>
        <label>Modèle local<input value={model} onChange={(e) => setModel(e.target.value)} spellCheck={false} /></label>
        <label>Déchargement (minutes)<input type="number" min={1} value={minutes} onChange={(e) => setMinutes(e.target.value)} /></label>
        <label>Chemin de Claude{status?.claude.kind === "found" && <small> (trouvé : {status.claude.path})</small>}
          <input value={claudePath} onChange={(e) => setClaudePath(e.target.value)} spellCheck={false} placeholder="Détection automatique" /></label>
        <label>Modèle Claude<input value={claudeModel} onChange={(e) => setClaudeModel(e.target.value)} spellCheck={false} placeholder="Par défaut" /></label>
        <button type="submit" className="acc-btn primary">Enregistrer les cerveaux</button>
      </form>
      {note}
    </>
  );
}

function Personality() {
  const s = useSettings();
  const [text, setText] = useState(s.iris_personality);
  const [save, note] = useSave();
  useEffect(() => setText(s.iris_personality), [s.iris_personality]);
  return (
    <>
      <h3>Personnalité</h3>
      <textarea className="iris-area" aria-label="Personnalité d'IRIS" rows={5} maxLength={MAX_PERSONALITY} value={text} onChange={(e) => setText(e.target.value)} />
      <div className="iris-row">
        <small>{text.length} / {MAX_PERSONALITY}</small>
        <button type="button" className="acc-btn" onClick={() => setText(DEFAULT_SETTINGS.iris_personality)}>Rétablir</button>
        <button type="button" className="acc-btn primary" onClick={() => void save({ iris_personality: text })}>Enregistrer la personnalité</button>
      </div>
      {note}
    </>
  );
}

function Security() {
  const s = useSettings();
  const [cmds, setCmds] = useState(s.iris_shell_allowlist.join("\n"));
  const [dirs, setDirs] = useState(s.iris_work_dirs.join("\n"));
  const [save, note] = useSave();
  useEffect(() => { setCmds(s.iris_shell_allowlist.join("\n")); setDirs(s.iris_work_dirs.join("\n")); }, [s]);
  return (
    <>
      <h3>Sécurité</h3>
      <label className="iris-field">Commandes autorisées (une par ligne)
        <textarea className="iris-area" rows={4} value={cmds} onChange={(e) => setCmds(e.target.value)} spellCheck={false} /></label>
      <label className="iris-field">Dossiers de travail (un par ligne)
        <textarea className="iris-area" rows={3} value={dirs} onChange={(e) => setDirs(e.target.value)} spellCheck={false} /></label>
      <button type="button" className="acc-btn primary" onClick={() => void save({ iris_shell_allowlist: lines(cmds), iris_work_dirs: lines(dirs) })}>Enregistrer la sécurité</button>
      {note}
    </>
  );
}

function Memories() {
  const [list, setList] = useState<Memory[]>([]);
  const load = useCallback(() => { irisIpc.memories().then(setList, () => {}); }, []);
  useEffect(load, [load]);
  return (
    <>
      <h3>Mémoire</h3>
      {list.length === 0 ? <p className="acc-empty">Aucune préférence retenue.</p> : (
        <ul className="acc-list">{list.map((m) => (
          <li key={m.id} className="acc-row"><div className="acc-main"><span className="acc-label">{m.text}</span>
            <button type="button" className="acc-icon" aria-label={`Oublier : ${m.text}`} onClick={() => void irisIpc.memoryDelete(m.id).finally(load)}><Trash2 size={14} /></button></div></li>
        ))}</ul>
      )}
    </>
  );
}

function History() {
  const [list, setList] = useState<HistoryItem[]>([]);
  const [confirm, setConfirm] = useState(false);
  const load = useCallback(() => { irisIpc.history(50).then(setList, () => {}); }, []);
  useEffect(load, [load]);
  return (
    <>
      <h3>Historique</h3>
      {list.length === 0 ? <p className="acc-empty">Aucun message.</p> : (
        <ul className="iris-hist">{list.map((h, i) => <li key={i}><small>{h.role === "user" ? "Toi" : "IRIS"}</small> {h.shown_text}</li>)}</ul>
      )}
      {!confirm
        ? <button type="button" className="acc-btn" onClick={() => setConfirm(true)}>Tout effacer</button>
        : (
          <div className="iris-row" role="alertdialog" aria-label="Confirmer l'effacement">
            <small>Efface les messages ; les préférences retenues (Mémoire) restent.</small>
            <button type="button" className="acc-btn primary" onClick={() => void irisIpc.historyClear().finally(() => { setConfirm(false); load(); })}>Confirmer l'effacement</button>
            <button type="button" className="acc-btn" onClick={() => setConfirm(false)}>Garder</button>
          </div>
        )}
    </>
  );
}

/** Feuille « IRIS » : cerveaux, personnalité, mémoire, sécurité, historique. Chargée à l'ouverture, sans sondage. */
export function IrisSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [status, setStatus] = useState<IrisStatus | null>(null);
  const root = useRef<HTMLElement>(null);
  const reload = useCallback(() => { irisIpc.status().then(setStatus, () => {}); }, []);
  useEffect(() => { if (open) { root.current?.focus(); reload(); } }, [open, reload]);
  if (!open) return null;
  return (
    <section ref={root} tabIndex={-1} className="acc-sheet" role="dialog" aria-modal="true" aria-label="IRIS"
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      <header className="acc-head"><h2>IRIS</h2>
        <button type="button" className="acc-icon" aria-label="Fermer IRIS" onClick={onClose}><X size={16} /></button></header>
      <div className="acc-body">
        <Brains status={status} reload={reload} />
        <Personality />
        <Memories />
        <Security />
        <History />
      </div>
    </section>
  );
}
