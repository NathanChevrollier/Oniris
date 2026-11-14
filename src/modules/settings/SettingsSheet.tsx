import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Brain, Check, ChevronRight, ClipboardList, FolderSearch, Keyboard, Mic, Plus, RefreshCw, Settings2, Sparkles, Sun, UserRound, X, type LucideIcon } from "lucide-react";
import { hotkeyText, useSettings, type HotkeySetting } from "../../lib/settings";
import { projectsIpc } from "../projects/api";
import { HiddenProjects } from "../projects/HiddenProjects";
import { IrisBrains, IrisHistory, IrisMemories, IrisPersonality, IrisSecurity } from "../iris/IrisSheet";
import { VoiceSection } from "../voice/VoiceSection";
import { Area, Field, Group, Row, SetCtx, Toggle, useDraft, useSaver, useSet } from "./controls";
import { onUpdate, restartText, updateIpc, type UpdateStatus } from "./update";
import { Appearance } from "./Appearance";
import { useVoidClick } from "../../panel/surface";
import "../accounts/accounts.css";
import "./settings.css";

/** Touche pressée vers réglage ; `null` pour un modificateur seul ou une touche que Rust refuse (`key_to_vk`). */
export function hotkeyFromEvent(e: { code: string; altKey: boolean; ctrlKey: boolean; shiftKey: boolean; metaKey: boolean }): HotkeySetting | null {
  const key = e.code === "Space" ? "Space" : /^Key[A-Z]$/.test(e.code) ? e.code.slice(3) : /^F([1-9]|1[0-2])$/.test(e.code) ? e.code : null;
  return key ? { alt: e.altKey, ctrl: e.ctrlKey, shift: e.shiftKey, win: e.metaKey, key } : null;
}

/** Liste de dossiers (aussi utilisée par l'accueil). */
export function Folders({ label, desc, value, onChange }: { label: string; desc?: string; value: string[]; onChange: (v: string[]) => void }) {
  const add = async () => { const p = await projectsIpc.browse().catch(() => null); if (p && !value.includes(p)) onChange([...value, p]); };
  return (
    <div className="iris-field set-wide set-folders"><b>{label}</b>
      {desc && <p className="set-desc">{desc}</p>}
      {value.length > 0 && <ul className="acc-list">{value.map((p) => (
        <li key={p} className="acc-row"><div className="acc-main"><span className="acc-label" title={p}>{p}</span>
          <button type="button" className="acc-icon" aria-label={`Retirer ${p}`} onClick={() => onChange(value.filter((x) => x !== p))}><X size={14} /></button></div></li>
      ))}</ul>}
      <button type="button" className="acc-btn" aria-label={`Ajouter un dossier : ${label}`} onClick={() => void add()}><Plus size={13} />Ajouter un dossier</button>
    </div>
  );
}

function FolderRow({ k, label, desc }: { k: "search_roots" | "projects_roots"; label: string; desc: string }) {
  const { s, save } = useSet();
  const [d, setD] = useDraft(s[k]);
  return <div className="set-row stack"><Folders label={label} desc={desc} value={d} onChange={(v) => { setD(v); void save({ [k]: v }); }} /></div>;
}

/** Capture du raccourci : enregistré dès qu'une combinaison valide est pressée. */
function HotkeyRow() {
  const { s, save } = useSet();
  const [d, setD] = useDraft(s.hotkey);
  return (
    <Row label="Raccourci" id="set-hotkey" desc="Clique dans le champ puis appuie sur la combinaison voulue.">
      <input id="set-hotkey" readOnly className="set-input set-key" value={hotkeyText(d)} aria-describedby="set-hotkey-d"
        onKeyDown={(e) => { const h = hotkeyFromEvent(e.nativeEvent); if (h) { e.preventDefault(); setD(h); void save({ hotkey: h }); } }} />
    </Row>
  );
}

const Notes = ({ text }: { text: string }) => <span style={{ whiteSpace: "pre-line" }}>{text}</span>;

/** Version installée, vérification manuelle, téléchargement et mise à jour en attente, suivis en direct. */
function UpdateInfo() {
  const [st, setSt] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [checked, setChecked] = useState(false);
  useEffect(() => {
    const un = onUpdate(setSt);
    void updateIpc.status().then((s) => setSt((v) => v ?? s)).catch(() => {});
    return () => { void un.then((f) => f()); };
  }, []);
  const check = async () => {
    setBusy(true);
    try { setSt(await updateIpc.checkNow()); setChecked(true); } catch { /* l'échec est journalisé côté Rust */ } finally { setBusy(false); }
  };
  const working = busy || st?.state === "checking" || st?.state === "downloading";
  const live = st?.state === "downloading" ? `Téléchargement de la mise à jour ${st.version}…${st.percent !== null ? ` ${st.percent} %` : ""}` : st && restartText(st);
  return (
    <>
      <Row label={st ? `Version ${st.current}` : "Version installée"}
        desc={live ? <span role="status">{live}</span>
          : st?.last_error && (!st.pending || st.state === "error") ? <span className="err" role="alert">{st.last_error}</span>
            : checked && !working && !st?.pending ? <span className="set-ok" role="status">Oniris est à jour.</span> : undefined}>
        <button type="button" className="acc-btn" disabled={working} onClick={() => void check()}>
          <RefreshCw size={13} className={working ? "set-spin" : undefined} />
          {working ? "Recherche…" : st?.state === "error" && !st.pending ? "Réessayer" : "Vérifier maintenant"}</button>
      </Row>
      {st?.pending && (
        <Row label={`Version ${st.pending[0]} prête`} desc={st.pending[1] ? <Notes text={st.pending[1]} /> : undefined}>
          {!live && <button type="button" className="acc-btn primary" onClick={() => void updateIpc.restart().catch(() => {})}>
            {st.state === "error" ? "Réessayer l'installation" : "Redémarrer pour l'installer"}</button>}
        </Row>
      )}
      {st?.updated && (
        <Row label={`Nouveautés de la version ${st.updated.version}`} desc={<Notes text={st.updated.notes || "Pas de notes pour cette version."} />}>
          <button type="button" className="acc-btn" onClick={() => void updateIpc.whatsNew().catch(() => {})}>Voir sur GitHub</button>
        </Row>
      )}
    </>
  );
}

export type SettingsCat = "general" | "pill" | "day" | "iris" | "voice" | "memory" | "clipboard" | "search" | "updates";

const CATS: { id: SettingsCat; icon: LucideIcon; title: string; intro: string; pane: () => ReactNode }[] = [
  { id: "general", icon: Settings2, title: "Général", intro: "Ton prénom, le démarrage avec Windows et les animations.", pane: () => <>
    <Group title="Toi"><Field k="user_name" label="Prénom" desc="IRIS et l'accueil t'appellent ainsi." /></Group>
    <Group title="Démarrage et affichage">
      <Toggle k="autostart" label="Lancer Oniris au démarrage de Windows" />
      <Toggle k="reduce_motion" label="Réduire les animations" desc="Moins de mouvement dans toute l'app, comme le réglage de Windows." />
    </Group>
    <Appearance />
  </> },
  { id: "pill", icon: Keyboard, title: "Pilule et raccourci", intro: "Comment ouvrir Oniris : au clavier ou en passant sur la pilule en haut de l'écran.", pane: () => <>
    <Group title="Raccourci">
      <HotkeyRow />
      <Field k="hold_threshold_ms" label="Appui long" unit="ms" desc="Un appui bref ouvre le panneau ; maintenu plus longtemps, le raccourci fait parler IRIS." />
      <Toggle k="win_key_opens_panel" label="Touche Windows" desc="Un appui seul sur Windows ouvre Oniris au lieu du menu Démarrer ; Win+E, Win+D et les autres combinaisons restent inchangées." />
    </Group>
    <Group title="Pilule">
      <Toggle k="hover_open" label="Ouvrir au survol" desc="Le panneau s'ouvre quand le curseur reste sur la pilule." />
      <Field k="hover_delay_ms" label="Délai de survol" unit="ms" />
      <Toggle k="hide_pill_in_fullscreen" label="Masquer en plein écran" desc="Pendant un jeu ou une vidéo en plein écran." />
      <Toggle k="pill_sleep" label="Pilule en veille" desc="Cachée, elle réapparaît quand le curseur touche le haut de l'écran, au milieu." />
    </Group>
  </> },
  { id: "day", icon: Sun, title: "Journée", intro: "La frise du jour, le minuteur, la météo et la relève des mails.", pane: () => <>
    <Group title="Frise du jour">
      <Field k="timeline_start_hour" label="Début" unit="h" min={0} max={23} />
      <Field k="timeline_end_hour" label="Fin" unit="h" min={1} max={24} />
    </Group>
    <Group title="Minuteur">
      <Field k="focus_minutes" label="Concentration" unit="min" />
      <Field k="break_minutes" label="Pause" unit="min" />
    </Group>
    <Group title="Météo et mails">
      <Field k="weather_city" label="Ville de la météo" placeholder="Vide : météo désactivée" />
      <Field k="mail_poll_minutes" label="Relève des mails" unit="min" min={2} max={5} desc="Toutes les 2 à 5 minutes." />
    </Group>
  </> },
  { id: "iris", icon: Sparkles, title: "IRIS", intro: "Les cerveaux d'IRIS, sa personnalité et ce qu'elle peut faire sans te demander.", pane: () => <><IrisBrains /><IrisPersonality /><IrisSecurity /></> },
  { id: "voice", icon: Mic, title: "Voix", intro: "Parler à IRIS et l'entendre répondre, avec ou sans la voix naturelle d'Azure.", pane: () => <VoiceSection /> },
  { id: "memory", icon: Brain, title: "Mémoire et historique", intro: "Ce qu'IRIS retient de toi et vos derniers échanges.", pane: () => <><IrisMemories /><IrisHistory /></> },
  { id: "clipboard", icon: ClipboardList, title: "Presse-papiers", intro: "L'historique de ce que tu copies, dans la carte Presse-papiers.", pane: () => (
    <Group>
      <Toggle k="clipboard_enabled" label="Garder l'historique" desc="Oniris retient ce que tu copies pour te le reproposer." />
      <Field k="clipboard_retention_days" label="Conservation" unit="jours" min={1} max={90} />
      <Field k="clipboard_max_items" label="Éléments au plus" min={20} max={1000} />
    </Group>
  ) },
  { id: "search", icon: FolderSearch, title: "Recherche et projets", intro: "Les dossiers que la recherche parcourt, tes projets et la reprise avec Claude Code.", pane: () => <>
    <Group title="Dossiers">
      <FolderRow k="search_roots" label="Dossiers de recherche" desc="Vide : ton dossier utilisateur." />
      <FolderRow k="projects_roots" label="Dossiers de projets" desc="Listés dans la carte Projets et indexés en plus." />
    </Group>
    <Group title="Reprendre avec Claude Code">
      <Area k="resume_prompt_normal" label="Invite « Reprendre »" desc="Envoyée à Claude Code quand tu reprends un projet." />
      <Area k="resume_prompt_orchestrator" label="Invite « Reprendre » (orchestrateur)" />
    </Group>
    <section className="set-group"><HiddenProjects /></section>
  </> },
  { id: "updates", icon: RefreshCw, title: "Mises à jour", intro: "Oniris se met à jour tout seul depuis GitHub, sans te déranger.", pane: () => (
    <Group>
      <Toggle k="auto_update" label="Mises à jour automatiques"
        desc="Oniris vérifie s'il existe une nouvelle version peu après son démarrage, toutes les 2 h, au réveil du PC et à l'ouverture du panneau (au plus une fois par heure), la télécharge en arrière-plan et l'installe quand tu fermes l'app ou cliques sur « Redémarrer », jamais pendant que tu joues ou que tu parles à IRIS. Désactivé : plus de vérification ni de téléchargement ; une mise à jour déjà téléchargée s'installe quand même à la fermeture." />
      <UpdateInfo />
    </Group>
  ) },
];

function Body({ cat, setCat, onClose, onAccounts }: { cat: SettingsCat; setCat: (c: SettingsCat) => void; onClose: () => void; onAccounts: () => void }) {
  const s = useSettings();
  const [save, msg, clear] = useSaver();
  const current = CATS.find((c) => c.id === cat) ?? CATS[0];
  const go = (c: SettingsCat) => { if (c !== cat) { clear(); setCat(c); } };
  // Flèches, Début et Fin : navigation verticale des catégories, le focus suit la sélection.
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    const i = CATS.findIndex((c) => c.id === cat);
    const j = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: CATS.length - 1 }[e.key];
    if (j === undefined) return;
    e.preventDefault();
    const next = CATS[(j + CATS.length) % CATS.length].id;
    go(next);
    e.currentTarget.querySelector<HTMLElement>(`#set-tab-${next}`)?.focus();
  };
  return (
    <SetCtx.Provider value={{ s, save }}>
      <nav className="set-nav" aria-label="Catégories des réglages" data-void>
        <h2>Réglages</h2>
        <div role="tablist" aria-orientation="vertical" aria-label="Catégories" onKeyDown={keys}>
          {CATS.map(({ id, icon: Icon, title }) => (
            <button key={id} id={`set-tab-${id}`} type="button" role="tab" className="set-tab" aria-selected={id === cat} aria-controls="set-panel"
              tabIndex={id === cat ? 0 : -1} title={title} onClick={() => go(id)}><Icon size={16} /><span>{title}</span></button>
          ))}
        </div>
        <div className="set-sep" />
        <button type="button" className="set-tab" title="Comptes" onClick={onAccounts}><UserRound size={16} /><span>Comptes</span><ChevronRight size={14} className="set-go" /></button>
      </nav>
      <div className="set-main">
        <header className="set-head" data-void>
          <div className="set-title"><h3>{current.title}</h3><p>{current.intro}</p></div>
          {msg && (msg.ok
            ? <span key={msg.n} className="set-saved" role="status"><Check size={13} />{msg.text}</span>
            : <p key={msg.n} className="err set-alert" role="alert">{msg.text}</p>)}
          <button type="button" className="acc-icon" aria-label="Fermer les réglages" onClick={onClose}><X size={16} /></button>
        </header>
        <div className="set-scroll" data-void>
          <div key={cat} id="set-panel" role="tabpanel" aria-labelledby={`set-tab-${cat}`} className="set-pane">{current.pane()}</div>
        </div>
      </div>
    </SetCtx.Provider>
  );
}

/** Fenêtre « Réglages » : catégories à gauche, réglages enregistrés automatiquement à droite. Comptes garde sa feuille. */
export function SettingsSheet({ open, section = "general", onClose, onAccounts }: { open: boolean; section?: SettingsCat; onClose: () => void; onAccounts: () => void }) {
  const [cat, setCat] = useState(section);
  const [wasOpen, setWasOpen] = useState(open);
  if (wasOpen !== open) { setWasOpen(open); if (open) setCat(section); }
  const root = useRef<HTMLElement>(null);
  // Clic dans le vide de la feuille (marges de la barre latérale, de l'en-tête ou sous le contenu) : retour au panneau.
  const voidClick = useVoidClick(onClose);
  useEffect(() => { if (open) root.current?.focus(); }, [open]);
  if (!open) return null;
  return (
    <section ref={root} tabIndex={-1} className="acc-sheet set-sheet" role="dialog" aria-modal="true" aria-label="Réglages" data-void {...voidClick}
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      <Body cat={cat} setCat={setCat} onClose={onClose} onAccounts={onAccounts} />
    </section>
  );
}
