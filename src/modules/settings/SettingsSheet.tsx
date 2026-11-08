import { useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { useSettings, type HotkeySetting, type Settings } from "../../lib/settings";
import { useSave } from "../../lib/useSave";
import { projectsIpc } from "../projects/api";
import { HiddenProjects } from "../projects/HiddenProjects";
import "../accounts/accounts.css";
import "../iris/iris.css";
import "../voice/voice.css";
import "./settings.css";

/** Brouillon d'une section : réinitialisé seulement quand l'un de SES champs change côté réglages. */
function Section<K extends keyof Settings>({ title, keys, children }: {
  title: string; keys: K[]; children: (d: Pick<Settings, K>, set: (p: Partial<Pick<Settings, K>>) => void) => ReactNode;
}) {
  const s = useSettings();
  const src = Object.fromEntries(keys.map((k) => [k, s[k]])) as Pick<Settings, K>;
  const key = JSON.stringify(src);
  const [d, setD] = useState(src);
  useEffect(() => setD(src), [key]); // eslint-disable-line react-hooks/exhaustive-deps -- `key` résume `src`
  const [save, note] = useSave();
  return (
    <>
      <h3>{title}</h3>
      <form className="iris-grid" onSubmit={(e) => { e.preventDefault(); void save(d); }}>
        {children(d, (p) => setD({ ...d, ...p }))}
        <button type="submit" className="acc-btn primary">Enregistrer · {title}</button>
      </form>
      {note}
    </>
  );
}

const check = (label: string, on: boolean, set: (v: boolean) => void) =>
  <label className="voice-check"><input type="checkbox" checked={on} onChange={(e) => set(e.target.checked)} />{label}</label>;
const num = (label: string, v: number, set: (n: number) => void, min?: number, max?: number) =>
  <label>{label}<input type="number" min={min} max={max} value={v} onChange={(e) => set(Number(e.target.value))} /></label>;

/** Touche pressée vers réglage ; `null` pour un modificateur seul ou une touche que Rust refuse (`key_to_vk`). */
export function hotkeyFromEvent(e: { code: string; altKey: boolean; ctrlKey: boolean; shiftKey: boolean; metaKey: boolean }): HotkeySetting | null {
  const key = e.code === "Space" ? "Space" : /^Key[A-Z]$/.test(e.code) ? e.code.slice(3) : /^F([1-9]|1[0-2])$/.test(e.code) ? e.code : null;
  return key ? { alt: e.altKey, ctrl: e.ctrlKey, shift: e.shiftKey, win: e.metaKey, key } : null;
}
const hotkeyText = (h: HotkeySetting) => [h.ctrl && "Ctrl", h.alt && "Alt", h.shift && "Maj", h.win && "Win", h.key].filter(Boolean).join(" + ");

export function Folders({ label, value, onChange }: { label: string; value: string[]; onChange: (v: string[]) => void }) {
  const add = async () => { const p = await projectsIpc.browse().catch(() => null); if (p && !value.includes(p)) onChange([...value, p]); };
  return (
    <div className="iris-field set-wide"><b>{label}</b>
      <ul className="acc-list">{value.map((p) => (
        <li key={p} className="acc-row"><div className="acc-main"><span className="acc-label">{p}</span>
          <button type="button" className="acc-icon" aria-label={`Retirer ${p}`} onClick={() => onChange(value.filter((x) => x !== p))}><X size={14} /></button></div></li>
      ))}</ul>
      <button type="button" className="acc-btn" aria-label={`Ajouter un dossier : ${label}`} onClick={() => void add()}>Ajouter un dossier</button>
    </div>
  );
}

/** Feuille « Paramètres » : réglages généraux ; IRIS, voix, comptes et sites gardent leurs feuilles. */
export function SettingsSheet({ open, onClose, onGo }: { open: boolean; onClose: () => void; onGo: (sheet: "iris" | "accounts") => void }) {
  const root = useRef<HTMLElement>(null);
  useEffect(() => { if (open) root.current?.focus(); }, [open]);
  if (!open) return null;
  return (
    <section ref={root} tabIndex={-1} className="acc-sheet" role="dialog" aria-modal="true" aria-label="Paramètres"
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      <header className="acc-head"><h2>Paramètres</h2>
        <button type="button" className="acc-icon" aria-label="Fermer les paramètres" onClick={onClose}><X size={16} /></button></header>
      <div className="acc-body">
        <p className="set-links">Cerveaux, voix et mémoire d'IRIS : <button type="button" className="acc-btn" onClick={() => onGo("iris")}>Ouvrir IRIS</button>
          Comptes : <button type="button" className="acc-btn" onClick={() => onGo("accounts")}>Ouvrir Comptes</button></p>
        <Section title="Général" keys={["user_name", "autostart", "reduce_motion", "hide_pill_in_fullscreen"]}>
          {(d, set) => <>
            <label>Prénom<input value={d.user_name} onChange={(e) => set({ user_name: e.target.value })} /></label>
            {check("Lancer Oniris au démarrage de Windows", d.autostart, (v) => set({ autostart: v }))}
            {check("Réduire les animations", d.reduce_motion, (v) => set({ reduce_motion: v }))}
            {check("Masquer la pilule en plein écran", d.hide_pill_in_fullscreen, (v) => set({ hide_pill_in_fullscreen: v }))}
          </>}
        </Section>
        <Section title="Raccourci" keys={["hotkey", "hold_threshold_ms", "hover_open", "hover_delay_ms"]}>
          {(d, set) => <>
            <label>Raccourci (appuie sur la combinaison)
              <input readOnly aria-label="Raccourci" value={hotkeyText(d.hotkey)}
                onKeyDown={(e) => { const h = hotkeyFromEvent(e.nativeEvent); if (h) { e.preventDefault(); set({ hotkey: h }); } }} /></label>
            {num("Maintien (ms)", d.hold_threshold_ms, (n) => set({ hold_threshold_ms: n }))}
            {check("Ouvrir au survol de la pilule", d.hover_open, (v) => set({ hover_open: v }))}
            {num("Délai de survol (ms)", d.hover_delay_ms, (n) => set({ hover_delay_ms: n }))}
          </>}
        </Section>
        <Section title="Journée" keys={["timeline_start_hour", "timeline_end_hour", "focus_minutes", "break_minutes", "weather_city"]}>
          {(d, set) => <>
            {num("Début de la frise (h)", d.timeline_start_hour, (n) => set({ timeline_start_hour: n }), 0, 23)}
            {num("Fin de la frise (h)", d.timeline_end_hour, (n) => set({ timeline_end_hour: n }), 1, 24)}
            {num("Concentration (min)", d.focus_minutes, (n) => set({ focus_minutes: n }))}
            {num("Pause (min)", d.break_minutes, (n) => set({ break_minutes: n }))}
            <label>Ville de la météo<input value={d.weather_city} placeholder="Vide : météo désactivée" onChange={(e) => set({ weather_city: e.target.value })} /></label>
          </>}
        </Section>
        <Section title="Presse-papiers" keys={["clipboard_enabled", "clipboard_retention_days", "clipboard_max_items"]}>
          {(d, set) => <>
            {check("Garder l'historique", d.clipboard_enabled, (v) => set({ clipboard_enabled: v }))}
            {num("Conservation (jours)", d.clipboard_retention_days, (n) => set({ clipboard_retention_days: n }), 1, 90)}
            {num("Éléments au plus", d.clipboard_max_items, (n) => set({ clipboard_max_items: n }), 20, 1000)}
          </>}
        </Section>
        <Section title="Recherche et projets" keys={["search_roots", "projects_roots", "resume_prompt_normal", "resume_prompt_orchestrator"]}>
          {(d, set) => <>
            <Folders label="Dossiers de recherche" value={d.search_roots} onChange={(v) => set({ search_roots: v })} />
            <Folders label="Dossiers de projets" value={d.projects_roots} onChange={(v) => set({ projects_roots: v })} />
            <label className="set-wide">Invite « Reprendre »
              <textarea className="iris-area" rows={3} value={d.resume_prompt_normal} onChange={(e) => set({ resume_prompt_normal: e.target.value })} /></label>
            <label className="set-wide">Invite « Reprendre » (orchestrateur)
              <textarea className="iris-area" rows={3} value={d.resume_prompt_orchestrator} onChange={(e) => set({ resume_prompt_orchestrator: e.target.value })} /></label>
          </>}
        </Section>
        <HiddenProjects />
        <Section title="Mails" keys={["mail_poll_minutes"]}>
          {(d, set) => num("Relève (minutes)", d.mail_poll_minutes, (n) => set({ mail_poll_minutes: n }), 2, 5)}
        </Section>
      </div>
    </section>
  );
}
