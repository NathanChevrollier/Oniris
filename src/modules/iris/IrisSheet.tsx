import { useCallback, useEffect, useState } from "react";
import { ExternalLink, Trash2 } from "lucide-react";
import { DEFAULT_SETTINGS } from "../../lib/settings";
import { accountsIpc, errorText } from "../accounts/api";
import { Area, Field, Group, Row, Toggle, useSet } from "../settings/controls";
import { irisIpc, onIrisPull, type HistoryItem, type IrisStatus, type Memory } from "./api";
import "../accounts/accounts.css";

// Catégories « IRIS » et « Mémoire et historique » de la fenêtre Réglages.

const MAX_PERSONALITY = 2000;

/** Cerveau local (Ollama) et Claude Code : état, installation, modèle. L'état est relu quand le modèle ou le chemin change. */
export function IrisBrains() {
  const { s, save } = useSet();
  const [status, setStatus] = useState<IrisStatus | null>(null);
  const reload = useCallback(() => { irisIpc.status().then(setStatus, () => {}); }, []);
  useEffect(reload, [reload, s.iris_local_model, s.claude_path]);
  const [pull, setPull] = useState<{ done: number; total: number } | null>(null);
  const [pullError, setPullError] = useState<string | null>(null);
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
  const gb = status?.vram_mb ? Math.round(status.vram_mb / 1024) : null;
  const pct = pull && pull.total > 0 ? Math.round((pull.done / pull.total) * 100) : null;
  const claude = status?.claude.kind === "found" ? status.claude.path : null;
  return (
    <>
      <Group title="Cerveau local">
        <Row label="Ollama" desc="Le cerveau principal d'IRIS : il tourne sur ton PC, rien ne sort.">
          {status?.local === "ready" && <span className="set-chip">Prêt · {status.model}</span>}
          {status?.local === "down" && !status.ollama_installed && <><span className="set-chip warn">Ollama n'est pas installé</span>
            <button type="button" className="acc-btn primary" disabled={installing} onClick={install}>{installing ? "Installation… (1 à 2 min)" : "Installer Ollama"}</button></>}
          {status?.local === "down" && status.ollama_installed && <><span className="set-chip warn">Ollama ne répond pas</span>
            <button type="button" className="acc-btn primary" onClick={reload}>Relancer</button></>}
          {status?.local === "no_model" && <><span className="set-chip warn">Modèle absent</span>
            <button type="button" className="acc-btn primary" disabled={pull !== null} onClick={download}>Télécharger le modèle</button></>}
        </Row>
        {(installFailed || pull || pullError) && (
          <div className="set-row stack">
            {installFailed && <p className="err" role="alert">L'installation automatique n'a pas marché. <button type="button" className="acc-btn" onClick={() => void guide()}><ExternalLink size={13} />Télécharger Ollama</button></p>}
            {pull && <progress className="set-progress" aria-label="Téléchargement du modèle" max={pull.total || undefined} value={pull.total ? pull.done : undefined} />}
            {pull && <span role="status" className="set-desc">Téléchargement{pct !== null ? ` : ${pct} %` : "…"}</span>}
            {pullError && <p className="err" role="alert">{pullError}</p>}
          </div>
        )}
        {status && status.local !== "down" && status.model !== status.recommended && (
          <Row label={`Modèle conseillé pour ta carte graphique${gb ? ` (${gb} Go)` : ""}`} desc={status.recommended}>
            <button type="button" className="acc-btn" onClick={() => void save({ iris_local_model: status.recommended })}>Utiliser</button>
          </Row>
        )}
        <Field k="iris_local_model" label="Modèle local" desc="Nom du modèle Ollama, par exemple qwen3:14b." />
        <Field k="iris_unload_minutes" label="Libérer la mémoire après" unit="min" min={1} max={60} desc="Sans question pendant ce temps, le modèle quitte la mémoire de la carte graphique." />
      </Group>
      <Group title="Claude Code">
        <Row label="Claude Code" desc={claude ?? "Introuvable : installe Claude Code ou indique son chemin ci-dessous."}>
          {claude ? <span className="set-chip" title={claude}>Trouvé</span> : status && <span className="set-chip warn">Introuvable</span>}
        </Row>
        <Field k="claude_path" label="Chemin de Claude" placeholder="Détection automatique" wide />
        <Field k="iris_claude_model" label="Modèle Claude" placeholder="Par défaut" />
        <Toggle k="iris_claude_auto" label="IRIS peut appeler Claude d'elle-même"
          desc="Désactivé : Claude n'intervient que si tu le demandes (bouton Claude, « demande à Claude… ») ; sinon IRIS te le propose." />
      </Group>
    </>
  );
}

export function IrisPersonality() {
  const { save } = useSet();
  return (
    <Group title="Personnalité">
      <Area k="iris_personality" label="Personnalité d'IRIS" rows={5} max={MAX_PERSONALITY}
        desc="Le ton et les habitudes d'IRIS. {prénom} est remplacé par ton prénom."
        after={(d, set) => (
          <div className="set-bar">
            <small>{d.length} / {MAX_PERSONALITY}</small>
            <button type="button" className="acc-btn" onClick={() => { set(DEFAULT_SETTINGS.iris_personality); void save({ iris_personality: DEFAULT_SETTINGS.iris_personality }); }}>Rétablir</button>
          </div>
        )} />
    </Group>
  );
}

export function IrisSecurity() {
  return (
    <Group title="Sécurité" note="Tout le reste demande ta confirmation par une carte dans le panneau.">
      <Area k="iris_shell_allowlist" list rows={4} label="Commandes autorisées"
        desc="Commandes PowerShell qu'IRIS lance sans te demander, une par ligne. Une commande qui commence par l'une d'elles passe aussi." />
      <Area k="iris_work_dirs" list label="Dossiers de travail"
        desc="Dans ces dossiers, IRIS renomme, déplace ou supprime des fichiers sans carte de confirmation. Un chemin par ligne." />
    </Group>
  );
}

export function IrisMemories() {
  const [list, setList] = useState<Memory[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => { irisIpc.memories().then((l) => { setList(l); setErr(null); }, () => setErr("Impossible de charger les préférences.")); }, []);
  useEffect(load, [load]);
  return (
    <Group title="Préférences retenues" note="IRIS retient ce que tu lui dis de toi pour mieux t'aider. Retire ce qui ne te convient plus.">
      {err && <div className="set-row"><p className="err" role="alert">{err}</p></div>}
      {list.length === 0 && !err ? <div className="set-row"><p className="set-desc">Aucune préférence retenue.</p></div> : (
        <ul className="set-list">{list.map((m) => (
          <li key={m.id}><span>{m.text}</span>
            <button type="button" className="acc-icon" aria-label={`Oublier : ${m.text}`} onClick={() => void irisIpc.memoryDelete(m.id).then(load, (e) => setErr(errorText(e, "Suppression impossible.")))}><Trash2 size={14} /></button></li>
        ))}</ul>
      )}
    </Group>
  );
}

export function IrisHistory() {
  const [list, setList] = useState<HistoryItem[]>([]);
  const [confirm, setConfirm] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => { irisIpc.history(50).then((l) => { setList(l); setErr(null); }, () => setErr("Impossible de charger l'historique.")); }, []);
  useEffect(load, [load]);
  return (
    <Group title="Conversation">
      <Row label="Historique" desc="Les 50 derniers messages échangés avec IRIS.">
        {!confirm && <button type="button" className="acc-btn" onClick={() => setConfirm(true)}>Tout effacer</button>}
      </Row>
      {confirm && (
        <div className="set-row set-confirm" role="alertdialog" aria-label="Confirmer l'effacement">
          <p className="set-desc">Efface les messages ; les préférences retenues restent.</p>
          <div className="set-ctl">
            <button type="button" className="acc-btn" onClick={() => setConfirm(false)}>Garder</button>
            <button type="button" className="acc-btn primary" onClick={() => void irisIpc.historyClear().then(load, (e) => setErr(errorText(e, "Effacement impossible."))).finally(() => setConfirm(false))}>Confirmer l'effacement</button>
          </div>
        </div>
      )}
      {err && <div className="set-row"><p className="err" role="alert">{err}</p></div>}
      {list.length === 0 && !err ? <div className="set-row"><p className="set-desc">Aucun message.</p></div> : (
        <ul className="set-hist">{list.map((h, i) => <li key={i}><small>{h.role === "user" ? "Toi" : "IRIS"}</small>{h.shown_text}</li>)}</ul>
      )}
    </Group>
  );
}
