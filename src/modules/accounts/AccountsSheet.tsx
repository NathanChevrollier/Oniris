import { useEffect, useRef, useState, type FormEvent } from "react";
import { Check, Copy, ExternalLink, RefreshCw, Trash2, X } from "lucide-react";
import { useTopic } from "../../lib/data";
import { useSettings } from "../../lib/settings";
import { accountsIpc, COLOR_NAMES, errorText, PALETTE, type AccountView } from "./api";
import { GUIDES, type Guide, type Step } from "./guide";
import { useVoidClick } from "../../panel/surface";
import "./accounts.css";

const PROVIDER_NAME = { google: "Gmail", spotify: "Spotify" } as const;

function CopyButton({ label, value, hint }: { label: string; value: string; hint: string }) {
  const [done, setDone] = useState(false);
  const copy = async () => {
    try { await navigator.clipboard.writeText(value); setDone(true); setTimeout(() => setDone(false), 1500); } catch { /* presse-papiers refusé */ }
  };
  return (
    <button type="button" className="acc-btn" aria-label={`${label} : ${hint}`} onClick={() => void copy()}>
      {done ? <Check size={13} /> : <Copy size={13} />}{done ? "Copié" : label}
    </button>
  );
}

/** Identifiants de l'application créée par l'utilisateur ; le secret n'est jamais relu ni prérempli. */
function CredentialsForm({ kind, clientId, onSaved }: { kind: "google" | "spotify"; clientId: string; onSaved: () => void }) {
  const [id, setId] = useState(clientId);
  const [secret, setSecret] = useState("");
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  useEffect(() => { setId(clientId); }, [clientId]);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      if (kind === "google") await accountsIpc.setGoogleApp(id, secret);
      else await accountsIpc.setSpotifyApp(id);
      setSecret("");
      setMsg({ ok: true, text: "Enregistré." });
      onSaved();
    } catch (err) {
      setMsg({ ok: false, text: errorText(err, "Enregistrement impossible, réessaie.") });
    }
  };
  return (
    <form className="acc-form" onSubmit={(e) => void submit(e)}>
      <input aria-label={kind === "google" ? "Identifiant client Google" : "Client ID Spotify"} placeholder={kind === "google" ? "Identifiant client" : "Client ID"}
        value={id} onChange={(e) => setId(e.target.value)} autoComplete="off" spellCheck={false} />
      {kind === "google" && (
        <input type="password" aria-label="Secret client Google" placeholder="Secret client" value={secret}
          onChange={(e) => setSecret(e.target.value)} autoComplete="off" spellCheck={false} />
      )}
      <button type="submit" className="acc-btn primary">Enregistrer</button>
      {msg && <p className={msg.ok ? "acc-ok" : "err"} role={msg.ok ? "status" : "alert"}>{msg.text}</p>}
    </form>
  );
}

function GuideView({ guide, clientIds, onSaved }: { guide: Guide; clientIds: { google: string; spotify: string }; onSaved: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const open = (url: string) => accountsIpc.openGuide(url).then(() => setError(null), (e) => setError(errorText(e, "Ouverture impossible.")));
  const renderStep = (s: Step) => (
    <li key={s.text}>
      <p>{s.text}</p>
      {s.copy && (
        <div className="acc-copy">
          <code>{s.copy.value}</code>
          <CopyButton label={s.copy.label} value={s.copy.value} hint={s.text} />
        </div>
      )}
      {s.open && <button type="button" className="acc-btn" aria-label={`${s.open.label} : ${s.text}`} onClick={() => void open(s.open!.url)}><ExternalLink size={13} />{s.open.label}</button>}
      {s.form && <CredentialsForm kind={s.form} clientId={clientIds[s.form]} onSaved={onSaved} />}
    </li>
  );
  return (
    <details className="acc-guide" id={`acc-guide-${guide.id}`} name="acc-guides">
      <summary>{guide.title}</summary>
      <ol>{guide.steps.map(renderStep)}</ol>
      {guide.note && <p className="acc-note">{guide.note}</p>}
      {error && <p className="err" role="alert">{error}</p>}
    </details>
  );
}

function AccountRow({ a, busy, onReconnect, onChange }: { a: AccountView; busy: boolean; onReconnect: () => void; onChange: () => void }) {
  const [picking, setPicking] = useState(false);
  const act = (p: Promise<void>) => p.then(onChange, onChange);
  return (
    <li className="acc-row">
      <div className="acc-main">
        <button type="button" className="acc-dot" style={{ background: a.color }} aria-label={`Couleur de ${a.label}`} aria-expanded={picking} onClick={() => setPicking(!picking)} />
        <span className="acc-label">{a.label}<small>{PROVIDER_NAME[a.provider]}</small></span>
        <span className={a.status === "reauth" ? "acc-state warn" : "acc-state"}>{a.status === "reauth" ? "À reconnecter" : "Connecté"}</span>
        {a.status === "reauth" && <button type="button" className="acc-btn" disabled={busy} onClick={onReconnect}><RefreshCw size={13} />Reconnecter</button>}
        <button type="button" className="acc-icon" aria-label={`Supprimer ${a.label}`} onClick={() => void act(accountsIpc.remove(a.id))}><Trash2 size={14} /></button>
      </div>
      {picking && (
        <div className="acc-palette" role="group" aria-label="Couleurs">
          {PALETTE.map((c, i) => (
            <button key={c} type="button" className={c === a.color ? "acc-sw on" : "acc-sw"} style={{ background: c }} aria-label={`Couleur ${COLOR_NAMES[i]}`} aria-pressed={c === a.color}
              onClick={() => { setPicking(false); void act(accountsIpc.setColor(a.id, c)); }} />
          ))}
        </div>
      )}
    </li>
  );
}

/** Feuille « Comptes » par-dessus le panneau. S'ouvre seule une fois, au premier lancement sans compte ni identifiant client. */
export function AccountsSheet({ open, onClose, onAutoOpen }: { open: boolean; onClose: () => void; onAutoOpen: () => void }) {
  const settings = useSettings();
  const { data, reload } = useTopic<AccountView[] | null>("accounts", accountsIpc.list, null);
  const [busy, setBusy] = useState<AccountView["provider"] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLElement>(null);
  const introAsked = useRef(false);
  const voidClick = useVoidClick(onClose);

  useEffect(() => {
    if (data === null || introAsked.current) return;
    introAsked.current = true;
    accountsIpc.introTake().then((first) => { if (first) onAutoOpen(); }, () => {});
  }, [data, onAutoOpen]);
  useEffect(() => { if (open) root.current?.focus(); }, [open]);

  if (!open) return null;
  const googleReady = settings.google_client_id !== "";
  const spotifyReady = settings.spotify_client_id !== "";
  const connect = async (provider: AccountView["provider"]) => {
    setBusy(provider); setError(null);
    try { await accountsIpc.connect(provider); }
    catch (e) { const m = errorText(e, "Connexion impossible, réessaie."); if (m !== "Connexion annulée.") setError(m); }
    finally { setBusy(null); reload(); }
  };
  const configure = (id: "google" | "spotify") => {
    const d = document.getElementById(`acc-guide-${id}`) as HTMLDetailsElement | null;
    if (d) { d.open = true; d.scrollIntoView?.({ block: "start" }); }
  };
  const list = data ?? [];
  const add = (provider: AccountView["provider"], ready: boolean, label: string) => (
    <div className="acc-add">
      <button type="button" className="acc-btn primary" disabled={!ready || busy !== null} onClick={() => void connect(provider)}>{label}</button>
      {busy === provider && (
        <>
          <span role="status" className="acc-wait">En attente du navigateur…</span>
          <button type="button" className="acc-btn" onClick={() => void accountsIpc.cancel().catch(() => {})}>Annuler</button>
        </>
      )}
      {!ready && <button type="button" className="acc-link" onClick={() => configure(provider)}>Configurer d'abord</button>}
    </div>
  );
  return (
    <section ref={root} tabIndex={-1} className="acc-sheet" role="dialog" aria-modal="true" aria-label="Comptes" data-void {...voidClick}
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      <header className="acc-head" data-void>
        <h2>Comptes</h2>
        <button type="button" className="acc-icon" aria-label="Fermer les comptes" onClick={onClose}><X size={16} /></button>
      </header>
      <div className="acc-body">
        <h3>Comptes connectés</h3>
        {list.length === 0
          ? <p className="acc-empty">Aucun compte connecté pour l'instant.</p>
          : <ul className="acc-list">{list.map((a) => <AccountRow key={a.id} a={a} busy={busy !== null} onReconnect={() => void connect(a.provider)} onChange={reload} />)}</ul>}
        <div className="acc-adds">
          {add("google", googleReady, "Ajouter un compte Gmail")}
          {add("spotify", spotifyReady, "Connecter Spotify")}
        </div>
        {error && <p className="err" role="alert">{error}</p>}
        <h3>Guides pas à pas</h3>
        {GUIDES.map((g) => <GuideView key={g.id} guide={g} clientIds={{ google: settings.google_client_id, spotify: settings.spotify_client_id }} onSaved={reload} />)}
      </div>
    </section>
  );
}
