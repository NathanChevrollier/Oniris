import { useState } from "react";
import { Pencil, Plus, X } from "lucide-react";
import { useTopic } from "../../lib/data";
import { counter, sitesIpc, type SiteState, type SiteView } from "./api";
import "./sites.css";

const NONE: SiteView[] = [];
const LABEL: Record<SiteState, string> = { unknown: "pas encore sondé", up: "en ligne", slow: "lent", down: "en panne", offline: "hors ligne" };

/** Message lisible pour un refus du Rust (validation des réglages). */
export function addError(e: unknown): string {
  const m = String(e);
  if (m.includes("déjà surveillée")) return "Ce site est déjà surveillé.";
  if (m.includes("30 au plus")) return "30 sites au plus.";
  if (m.includes("adresse")) return "Adresse invalide, essaie https://exemple.com.";
  return "Ajout impossible, réessaie.";
}

/** Courbe des temps de réponse, sans axe : seule la forme compte. */
export function Spark({ values }: { values: number[] }) {
  if (values.length < 2) return <span className="site-spark" aria-hidden />;
  const [W, H] = [48, 14];
  const [lo, hi] = [Math.min(...values), Math.max(...values)];
  const pts = values.map((v, i) => `${((i / (values.length - 1)) * W).toFixed(1)},${(H - 1 - (hi === lo ? 0.5 : (v - lo) / (hi - lo)) * (H - 2)).toFixed(1)}`).join(" ");
  return (
    <svg className="site-spark" viewBox={`0 0 ${W} ${H}`} width={W} height={H} aria-hidden focusable="false">
      <polyline points={pts} fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}

/** Surveillance des sites : compteur, pastille, temps de réponse et courbe ; la liste s'édite ici. */
export function SitesSection() {
  const { data, reload } = useTopic<SiteView[]>("sites", sitesIpc.list, NONE);
  const [editing, setEditing] = useState(false);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const { online, total } = counter(data);
  const offline = data.length > 0 && data.every((s) => s.state === "offline");

  const add = async () => {
    if (!url.trim()) return;
    try { await sitesIpc.add(url, name); setUrl(""); setName(""); setError(null); reload(); } catch (e) { setError(addError(e)); }
  };
  const remove = async (s: SiteView) => {
    try { await sitesIpc.remove(s.url); setError(null); reload(); } catch { setError("Suppression impossible, réessaie."); }
  };

  return (
    <section className="sites" aria-label="Sites surveillés">
      <div className="sites-head">
        <p className="sites-count" aria-live="polite">{data.length === 0 ? "Sites" : offline ? "Hors ligne" : total === 0 ? "En attente…" : `${online}/${total} en ligne`}</p>
        <button type="button" className="sites-edit" aria-label={editing ? "Terminer la modification des sites" : "Modifier les sites"} aria-pressed={editing} onClick={() => { setEditing(!editing); setError(null); }}>
          <Pencil size={11} aria-hidden />
        </button>
      </div>
      {data.length === 0 && !editing && <p className="sites-empty">Aucun site surveillé</p>}
      {data.length > 0 && (
        <ul className="site-list" role="list">
          {data.map((s) => (
            <li key={s.url} className="site">
              <span className={`site-dot ${s.state}`} role="img" aria-label={LABEL[s.state]} />
              <span className="site-name">{s.name}</span>
              {editing
                ? <button type="button" className="site-del" aria-label={`Retirer ${s.name}`} onClick={() => void remove(s)}><X size={12} aria-hidden /></button>
                : <>
                    <Spark values={s.spark} />
                    <span className="site-ms">{s.state === "down" ? "—" : s.last_ms === null ? "" : s.last_ms >= 1000 ? `${(s.last_ms / 1000).toFixed(1)} s` : `${s.last_ms} ms`}</span>
                  </>}
            </li>
          ))}
        </ul>
      )}
      {editing && (
        <form className="site-form" onSubmit={(e) => { e.preventDefault(); void add(); }}>
          <input aria-label="Adresse du site à surveiller" placeholder="https://…" value={url} onChange={(e) => setUrl(e.target.value)} spellCheck={false} />
          <input aria-label="Nom (facultatif)" placeholder="Nom (facultatif)" value={name} onChange={(e) => setName(e.target.value)} spellCheck={false} />
          <button type="submit"><Plus size={12} aria-hidden /> Surveiller</button>
        </form>
      )}
      {error && <p className="err" role="alert">{error}</p>}
    </section>
  );
}
