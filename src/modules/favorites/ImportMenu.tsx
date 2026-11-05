import { useEffect, useRef, useState } from "react";
import { formatReport, importIpc } from "./api";

/** Bouton « Importer » : menu (navigateurs / export Nexus) et compte rendu en ligne. */
export function ImportMenu() {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const out = (e: Event) => { if (!root.current?.contains(e.target as Node)) setOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("pointerdown", out);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("pointerdown", out); document.removeEventListener("keydown", esc); };
  }, [open]);

  const go = async (job: () => Promise<string[]>) => {
    setOpen(false); setBusy(true); setError(null); setLines([]);
    try { setLines(await job()); }
    catch (e) { setError(typeof e === "string" ? e : "Import impossible, réessaie."); }
    finally { setBusy(false); }
  };
  const fromBrowsers = () => go(async () => {
    const r = await importIpc.browsers();
    return r.length ? r.map(([name, rep]) => `${name} : ${formatReport(rep)}`) : ["Aucun navigateur trouvé."];
  });
  const fromNexus = () => go(async () => { const r = await importIpc.nexusFile(); return r ? [formatReport(r)] : []; });

  return (
    <div className="fav-import" ref={root}>
      <button type="button" className="fav-import-btn" aria-haspopup="menu" aria-expanded={open} disabled={busy} onClick={() => setOpen(!open)}>
        {busy ? "Import…" : "Importer"}
      </button>
      {open && (
        <div className="fav-menu" role="menu">
          <button type="button" role="menuitem" autoFocus onClick={() => void fromBrowsers()}>Depuis les navigateurs</button>
          <button type="button" role="menuitem" onClick={() => void fromNexus()}>Depuis un export Nexus…</button>
        </div>
      )}
      <div aria-live="polite">{lines.map((l) => <p key={l} className="fav-report">{l}</p>)}</div>
      {error && <p className="err" role="alert">{error}</p>}
    </div>
  );
}
