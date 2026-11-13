import { useCallback, useEffect, useRef, useState } from "react";
import { ExternalLink, Trash2, X } from "lucide-react";
import { DEFAULT_SETTINGS, useSettings } from "../../lib/settings";
import { useSave } from "../../lib/useSave";
import { accountsIpc, errorText } from "../accounts/api";
import { irisIpc, onIrisPull, type HistoryItem, type IrisStatus, type Memory } from "./api";
import { VoiceSection } from "../voice/VoiceSection";
import "../accounts/accounts.css";
import "./iris.css";

const MAX_PERSONALITY = 2000;
const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);

function Brains({ status, reload }: { status: IrisStatus | null; reload: () => void }) {
  const s = useSettings();
  const [model, setModel] = useState(s.iris_local_model);
  const [minutes, setMinutes] = useState(String(s.iris_unload_minutes));
  const [claudePath, setClaudePath] = useState(s.claude_path);
  const [claudeModel, setClaudeModel] = useState(s.iris_claude_model);
  const [claudeAuto, setClaudeAuto] = useState(s.iris_claude_auto);
  const [pull, setPull] = useState<{ done: number; total: number } | null>(null);
  const [pullError, setPullError] = useState<string | null>(null);
  const [save, note] = useSave();
  useEffect(() => { setModel(s.iris_local_model); setMinutes(String(s.iris_unload_minutes)); setClaudePath(s.claude_path); setClaudeModel(s.iris_claude_model); setClaudeAuto(s.iris_claude_auto); }, [s.iris_local_model, s.iris_unload_minutes, s.claude_path, s.iris_claude_model, s.iris_claude_auto]);
  useEffect(() => {
    const un = onIrisPull((e) => { if (e.error) setPullError(e.error); else setPull({ done: e.done ?? 0, total: e.total ?? 0 }); });
    return () => { void un.then((f) => f()); };
  }, []);
  const download = () => {
    setPullError(null); setPull({ done: 0, total: 0 });
    irisIpc.pullModel().catch((e) => setPullError(errorText(e, "Téléchargement impossible."))).finally(() => { setPull(null); reload(); });
  };
  const guide = () => accountsIpc.openGuide("https://ollama.com/download").catch((e) => setPullError(errorText(e, "Ouverture impossible.")));
  const [installing, setInstalling] = useState(false);
  const [installFailed, setInstallFailed] = useState(false);
  const install = () => {
    setInstalling(true); setInstallFailed(false); setPullError(null);
    irisIpc.installOllama().catch(() => setInstallFailed(true)).finally(() => { setInstalling(false); reload(); });
  };
  const useRecommended = () => { if (status) void save({ iris_local_model: status.recommended }).then(reload); };
  const gb = status?.vram_mb ? Math.round(status.vram_mb / 1024) : null;
  const pct = pull && pull.total > 0 ? Math.round((pull.done / pull.total) * 100) : null;
  return (
    <>
      <h3>Cerveaux</h3>
      <div className="iris-row">
        <b>Ollama (local)</b>
        {status?.local === "ready" && <span className="acc-state">Prêt · {status.model}</span>}
        {status?.local === "down" && !status.ollama_installed && <><span className="acc-state warn">Ollama n'est pas installé</span>
          <button type="button" className="acc-btn primary" disabled={installing} onClick={install}>{installing ? "Installation… (1 à 2 min)" : "Installer Ollama"}</button></>}
        {status?.local === "down" && status.ollama_installed && <><span className="acc-state warn">Ollama ne répond pas</span>
          <button type="button" className="acc-btn primary" onClick={reload}>Relancer</button></>}
        {status?.local === "no_model" && <><span className="acc-state warn">Modèle absent</span>
          <button type="button" className="acc-btn primary" disabled={pull !== null} onClick={download}>Télécharger le modèle</button></>}
      </div>
      {installFailed && <p className="err" role="alert">L'installation automatique n'a pas marché. <button type="button" className="acc-btn" onClick={() => void guide()}><ExternalLink size={13} />Télécharger Ollama</button></p>}
      {status && status.local !== "down" && status.model !== status.recommended && (
        <div className="iris-row"><small>Conseillé pour ta carte graphique{gb ? ` (${gb} Go)` : ""} : <b>{status.recommended}</b></small>
          <button type="button" className="acc-btn" onClick={useRecommended}>Utiliser</button></div>
      )}
      {pull && <progress className="iris-progress" aria-label="Téléchargement du modèle" max={pull.total || undefined} value={pull.total ? pull.done : undefined} />}
      {pull && <span role="status" className="acc-wait">Téléchargement{pct !== null ? ` : ${pct} %` : "…"}</span>}
      {pullError && <p className="err" role="alert">{pullError}</p>}
      <div className="iris-row">
        <b>Claude Code</b>
        {status?.claude.kind === "found"
          ? <span className="acc-state" title={status.claude.path}>Trouvé</span>
          : <span className="acc-state warn">Introuvable</span>}
      </div>
      <form className="iris-grid" onSubmit={(e) => { e.preventDefault(); void save({ iris_local_model: model, iris_unload_minutes: Number(minutes), claude_path: claudePath, iris_claude_model: claudeModel, iris_claude_auto: claudeAuto }).then(reload); }}>
        <label>Modèle local<input value={model} onChange={(e) => setModel(e.target.value)} spellCheck={false} /></label>
        <label>Déchargement (minutes)<input type="number" min={1} value={minutes} onChange={(e) => setMinutes(e.target.value)} /></label>
        <label>Chemin de Claude{status?.claude.kind === "found" && <small> (trouvé : {status.claude.path})</small>}
          <input value={claudePath} onChange={(e) => setClaudePath(e.target.value)} spellCheck={false} placeholder="Détection automatique" /></label>
        <label>Modèle Claude<input value={claudeModel} onChange={(e) => setClaudeModel(e.target.value)} spellCheck={false} placeholder="Par défaut" /></label>
        <label className="voice-check"><input type="checkbox" checked={claudeAuto} onChange={(e) => setClaudeAuto(e.target.checked)} />IRIS peut appeler Claude d'elle-même</label>
        <p className="voice-note">Décoché : Claude n'intervient que si tu le demandes (bouton Claude, « demande à Claude… »). Sinon IRIS te le propose.</p>
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
  const cmdsKey = s.iris_shell_allowlist.join("\n"), dirsKey = s.iris_work_dirs.join("\n");
  // Clés textuelles : seul un changement de ces champs réinitialise la section.
  useEffect(() => { setCmds(cmdsKey); setDirs(dirsKey); }, [cmdsKey, dirsKey]);
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
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => { irisIpc.memories().then((l) => { setList(l); setErr(null); }, () => setErr("Impossible de charger les préférences.")); }, []);
  useEffect(load, [load]);
  return (
    <>
      <h3>Mémoire</h3>
      {err && <p className="err" role="alert">{err}</p>}
      {list.length === 0 && !err ? <p className="acc-empty">Aucune préférence retenue.</p> : (
        <ul className="acc-list">{list.map((m) => (
          <li key={m.id} className="acc-row"><div className="acc-main"><span className="acc-label">{m.text}</span>
            <button type="button" className="acc-icon" aria-label={`Oublier : ${m.text}`} onClick={() => void irisIpc.memoryDelete(m.id).then(load, (e) => setErr(errorText(e, "Suppression impossible.")))}><Trash2 size={14} /></button></div></li>
        ))}</ul>
      )}
    </>
  );
}

function History() {
  const [list, setList] = useState<HistoryItem[]>([]);
  const [confirm, setConfirm] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => { irisIpc.history(50).then((l) => { setList(l); setErr(null); }, () => setErr("Impossible de charger l'historique.")); }, []);
  useEffect(load, [load]);
  return (
    <>
      <h3>Historique</h3>
      {err && <p className="err" role="alert">{err}</p>}
      {list.length === 0 && !err ? <p className="acc-empty">Aucun message.</p> : (
        <ul className="iris-hist">{list.map((h, i) => <li key={i}><small>{h.role === "user" ? "Toi" : "IRIS"}</small> {h.shown_text}</li>)}</ul>
      )}
      {!confirm
        ? <button type="button" className="acc-btn" onClick={() => setConfirm(true)}>Tout effacer</button>
        : (
          <div className="iris-row" role="alertdialog" aria-label="Confirmer l'effacement">
            <small>Efface les messages ; les préférences retenues (Mémoire) restent.</small>
            <button type="button" className="acc-btn primary" onClick={() => void irisIpc.historyClear().then(load, (e) => setErr(errorText(e, "Effacement impossible."))).finally(() => setConfirm(false))}>Confirmer l'effacement</button>
            <button type="button" className="acc-btn" onClick={() => setConfirm(false)}>Garder</button>
          </div>
        )}
    </>
  );
}

/** Feuille « IRIS » : cerveaux, voix, personnalité, mémoire, sécurité, historique. Chargée à l'ouverture, sans sondage. */
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
        <VoiceSection />
        <Personality />
        <Memories />
        <Security />
        <History />
      </div>
    </section>
  );
}
