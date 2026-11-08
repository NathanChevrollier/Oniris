import { useEffect, useRef, useState } from "react";
import { Folders } from "../settings/SettingsSheet";
import { useSettings } from "../../lib/settings";
import { useSave } from "../../lib/useSave";
import { errorText } from "../accounts/api";
import { onboardingIpc, type Onboarding } from "./api";
import "../accounts/accounts.css";
import "../iris/iris.css";
import "../settings/settings.css";
import "./onboarding.css";

const LAST = 5;

/** Premier lancement : écrans plein panneau, reprise à la dernière étape. Seul l'écran 2 ne se saute pas. */
export function Welcome({ covered, onActive, onGo }: { covered?: boolean; onActive?: (on: boolean) => void; onGo: (sheet: "accounts" | "iris") => void }) {
  const [ob, setOb] = useState<Onboarding | null>(null);
  const [suggested, setSuggested] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const [save, note] = useSave();
  const settings = useSettings();
  const [name, setName] = useState<string | null>(null);
  const [proj, setProj] = useState<string[] | null>(null);
  const [search, setSearch] = useState<string[] | null>(null);
  const root = useRef<HTMLElement>(null);
  const shown = ob != null && !ob.done;
  const step = ob ? Math.min(Math.max(ob.step, 1), LAST) : 1;

  useEffect(() => { onboardingIpc.get().then(({ suggested_root, ...s }) => { setOb(s); setSuggested(suggested_root); }, () => {}); }, []);
  useEffect(() => { onActive?.(shown); }, [shown, onActive]);
  useEffect(() => { if (shown) root.current?.focus(); }, [shown, step]);
  if (!ob || ob.done) return null;

  const go = async (patch: Partial<Onboarding>) => {
    const next = { ...ob, ...patch };
    try { await onboardingIpc.set(next); setOb(next); setErr(""); }
    catch (e) { setErr(errorText(e, "Enregistrement impossible.")); }
  };
  const next = () => go({ step: step + 1 });
  const finish = () => go({ step: LAST, done: true });
  const saveThen = async (patch: Parameters<typeof save>[0]) => { if (await save(patch)) await next(); };
  const skip = <button type="button" className="acc-btn" onClick={() => void next()}>Passer</button>;
  const projects = proj ?? (settings.projects_roots.length || !suggested ? settings.projects_roots : [suggested]);

  return (
    <section ref={root} tabIndex={-1} inert={covered} className="acc-sheet welcome" role="dialog" aria-modal="true" aria-label="Bienvenue"
      onKeyDown={(e) => { if (e.key === "Escape") e.stopPropagation(); }}>
      <header className="acc-head"><h2>Bienvenue dans Oniris</h2><span className="wl-dots">Étape {step} sur {LAST}</span></header>
      <div className="acc-body">
        {step === 1 && <>
          <p>Bonjour ! Comment dois-je t'appeler ?</p>
          <form className="iris-grid" onSubmit={(e) => { e.preventDefault(); void (name?.trim() ? saveThen({ user_name: name.trim() }) : next()); }}>
            <label>Prénom<input value={name ?? settings.user_name} onChange={(e) => setName(e.target.value)} /></label>
            <div className="wl-actions"><button type="submit" className="acc-btn primary">Continuer</button>{skip}</div>
          </form>
        </>}
        {step === 2 && <>
          <p>Lancer Oniris avec Windows ? Rien n'est inscrit dans Windows sans ta réponse ; tu pourras changer d'avis dans les paramètres.</p>
          <div className="wl-actions">
            <button type="button" className="acc-btn primary" onClick={() => void go({ autostart_consent: true, step: 3 })}>Oui</button>
            <button type="button" className="acc-btn" onClick={() => void go({ autostart_consent: false, step: 3 })}>Non</button>
          </div>
        </>}
        {step === 3 && <>
          <p>Où sont tes projets ? Oniris les liste et les indexe pour la recherche.</p>
          <Folders label="Dossiers de projets" value={projects} onChange={setProj} />
          <Folders label="Dossiers de recherche" value={search ?? settings.search_roots} onChange={setSearch} />
          <div className="wl-actions">
            <button type="button" className="acc-btn primary" onClick={() => void saveThen({ projects_roots: projects, search_roots: search ?? settings.search_roots })}>Continuer</button>{skip}
          </div>
        </>}
        {step === 4 && <>
          <p>Pour aller plus loin. Rien n'est installé ici : chaque feuille t'explique ce qu'elle fait.</p>
          <div className="wl-actions">
            <button type="button" className="acc-btn" onClick={() => onGo("accounts")}>Comptes (mail, agenda, Spotify)</button>
            <button type="button" className="acc-btn" onClick={() => onGo("iris")}>IRIS (Ollama)</button>
            <button type="button" className="acc-btn" onClick={() => onGo("iris")}>Voix</button>
          </div>
          <div className="wl-actions"><button type="button" className="acc-btn primary" onClick={() => void next()}>Continuer</button></div>
        </>}
        {step === 5 && <>
          <p>C'est prêt ! Appuie sur <kbd>Alt</kbd> + <kbd>Espace</kbd> pour ouvrir Oniris à tout moment.</p>
          <div className="wl-actions"><button type="button" className="acc-btn primary" onClick={() => void finish()}>Terminer</button></div>
        </>}
        {err && <p className="err" role="alert">{err}</p>}
        {note}
      </div>
    </section>
  );
}
