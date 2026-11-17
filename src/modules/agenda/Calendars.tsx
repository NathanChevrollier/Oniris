import { useState, type FormEvent } from "react";
import { FileUp, Link2, X } from "lucide-react";
import { useTopic } from "../../lib/data";
import { errorText } from "../accounts/api";
import { Group } from "../settings/controls";
import { calendarsIpc, reportText, type Calendar, type CalendarReport } from "./api";
import "../accounts/accounts.css";
import "./agenda.css";

const KIND_TITLE = { file: "Fichiers importés", url: "Abonnements en ligne" } as const;

function meta(c: Calendar): string | null {
  if (c.error) return `Dernière relève impossible : ${c.error}`;
  if (c.duplicate) return "Déjà affiché depuis un autre compte";
  if (c.primary) return "Principal";
  if (c.kind !== "google") return `${c.events} événement${c.events > 1 ? "s" : ""}`;
  return null;
}

/** Calendriers par compte (Google) puis importés et abonnements : case pour afficher ou masquer, suppression des locaux. */
export function CalendarList({ onError }: { onError: (m: string | null) => void }) {
  const { data, reload } = useTopic<Calendar[] | null>("agenda", calendarsIpc.list, null);
  const [busy, setBusy] = useState<number | null>(null);
  const act = async (id: number, job: () => Promise<void>) => {
    setBusy(id); onError(null);
    try { await job(); } catch (e) { onError(errorText(e, "Modification impossible, réessaie.")); } finally { setBusy(null); reload(); }
  };
  if (data === null) return <p className="agc-empty" role="status">Chargement des calendriers…</p>;
  if (data.length === 0) return <p className="agc-empty">Aucun calendrier pour l'instant : connecte un compte Google, importe un fichier .ics ou abonne-toi à un lien.</p>;
  const groups = new Map<string, Calendar[]>();
  for (const c of data) {
    const g = c.kind === "google" ? c.account ?? "Google" : KIND_TITLE[c.kind];
    groups.set(g, [...(groups.get(g) ?? []), c]);
  }
  return (
    <div className="agc">
      {[...groups].map(([title, cals]) => (
        <section key={title}>
          <h5>{title}</h5>
          <ul>
            {cals.map((c) => {
              const m = meta(c);
              return (
                <li key={c.id}>
                  <label title={c.url ?? undefined}>
                    <input type="checkbox" checked={c.visible} disabled={busy === c.id} onChange={(e) => void act(c.id, () => calendarsIpc.setVisible(c.id, e.target.checked))} />
                    <i className="agc-dot" style={{ background: c.color ?? "var(--txt3)" }} aria-hidden />
                    <span className="agc-name">{c.name}{m && <small className={c.error ? "err" : undefined}>{m}</small>}</span>
                  </label>
                  {c.kind !== "google" && (
                    <button type="button" className="agc-del" aria-label={`Supprimer le calendrier ${c.name}`} disabled={busy === c.id}
                      onClick={() => void act(c.id, () => calendarsIpc.remove(c.id))}><X size={13} /></button>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}

/** Import d'un fichier .ics et abonnement à un lien https/webcal ; compte rendu en ligne. */
export function CalendarAdd() {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [line, setLine] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const go = async (job: () => Promise<CalendarReport | null>) => {
    setBusy(true); setError(null); setLine(null);
    try { const r = await job(); if (r) setLine(reportText(r)); return r; }
    catch (e) { setError(errorText(e, "Import impossible, réessaie.")); return null; }
    finally { setBusy(false); }
  };
  const subscribe = async (e: FormEvent) => {
    e.preventDefault();
    const u = url.trim();
    if (u && (await go(() => calendarsIpc.subscribe(u)))) setUrl("");
  };
  return (
    <div className="agc-add">
      <button type="button" className="acc-btn" disabled={busy} onClick={() => void go(calendarsIpc.importFile)}><FileUp size={13} />Importer un fichier .ics</button>
      <form onSubmit={subscribe}>
        <input value={url} onChange={(e) => { setUrl(e.target.value); setError(null); }} placeholder="https://… ou webcal://… (.ics)"
          aria-label="Adresse d'un calendrier en ligne" autoComplete="off" spellCheck={false} disabled={busy} />
        <button type="submit" className="acc-btn" disabled={busy || !url.trim()}><Link2 size={13} />{busy ? "Ajout…" : "S'abonner"}</button>
      </form>
      <div aria-live="polite">{line && <p className="agc-ok">{line}</p>}</div>
      {error && <p className="err" role="alert">{error}</p>}
    </div>
  );
}

/** Liste et ajout réunis (Réglages et carte Agenda). */
export function CalendarsManager() {
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <CalendarList onError={setError} />
      {error && <p className="err" role="alert">{error}</p>}
      <CalendarAdd />
    </>
  );
}

/** Catégorie « Agenda » des Réglages. */
export function AgendaPane() {
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <Group title="Calendriers affichés" note="Un calendrier masqué n'est plus relevé. Un agenda partagé par plusieurs comptes (jours fériés…) n'est affiché qu'une fois par défaut, et un même événement venu de plusieurs calendriers n'apparaît qu'une fois.">
        <div className="set-row stack"><CalendarList onError={setError} />{error && <p className="err" role="alert">{error}</p>}</div>
      </Group>
      <Group title="Ajouter un calendrier" note="Un fichier importé reste sur ce PC, il n'est jamais envoyé à Google. Un calendrier en ligne (https) est relu au plus toutes les 6 h, panneau ouvert : seule l'adresse que tu donnes est contactée.">
        <div className="set-row stack"><CalendarAdd /></div>
      </Group>
    </>
  );
}
