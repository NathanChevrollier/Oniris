import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyEvent } from "react";
import { Download } from "lucide-react";
import { formatReport, importIpc } from "./api";

/** Bouton « Importer » : menu (navigateurs / export Nexus) et compte rendu en ligne. */
export function ImportMenu({ tile = false }: { tile?: boolean }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const out = (e: Event) => { if (!root.current?.contains(e.target as Node)) setOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") { setOpen(false); button.current?.focus(); } };
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
    return r.length ? r.map(([name, rep]) => `${name} : ${rep ? formatReport(rep) : "lecture impossible"}`) : ["Aucun navigateur trouvé."];
  });
  const fromNexus = () => go(async () => { const r = await importIpc.nexusFile(); return r ? [formatReport(r)] : []; });

  const arrows = (e: ReactKeyEvent<HTMLDivElement>) => {
    const dir = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!dir) return;
    e.preventDefault();
    const items = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    items[(items.indexOf(document.activeElement as HTMLElement) + dir + items.length) % items.length]?.focus();
  };

  return (
    <div className={`fav-import${tile ? " tile" : ""}`} ref={root}>
      <button type="button" ref={button} className={tile ? "fav-btn fav-add" : "fav-import-btn"} title={tile ? "Importer" : undefined} aria-haspopup="menu" aria-expanded={open} disabled={busy} onClick={() => setOpen(!open)}>
        {tile && <span className="fav-ico"><Download size={15} aria-hidden /></span>}
        {tile ? <span className="fav-t">{busy ? "Import…" : "Importer"}</span> : busy ? "Import…" : "Importer"}
      </button>
      {open && (
        <div className="fav-menu" role="menu" onKeyDown={arrows}>
          <button type="button" role="menuitem" autoFocus onClick={() => void fromBrowsers()}>Depuis les navigateurs</button>
          <button type="button" role="menuitem" onClick={() => void fromNexus()}>Depuis un export Nexus…</button>
        </div>
      )}
      <div aria-live="polite">{lines.map((l) => <p key={l} className="fav-report">{l}</p>)}</div>
      {error && <p className="err" role="alert">{error}</p>}
    </div>
  );
}
