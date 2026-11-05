import { useEffect, useRef, useState, type CSSProperties } from "react";
import { MoreHorizontal } from "lucide-react";
import { ago, type Project, type ResumeMode } from "./api";
import { hueOf } from "../favorites/FavoritesCard";
import { DARK, LIGHT, hsl } from "../favorites/contrast";

/** « MyEmpire » → ME, « MacroWorld » → MW, « Zenytt » → Ze, « mon-projet » → MP. */
export function badge(name: string): string {
  const parts = name.match(/[\p{Lu}\d][\p{Ll}\d]*|[\p{L}\d]+/gu) ?? [];
  const [a = "?", b] = parts;
  return (b ? [...a][0] + [...b][0] : [...a].slice(0, 2).join("")).replace(/^./, (c) => c.toUpperCase());
}

const C = 94.2; // 2π × 15 : circonférence de l'anneau

export function ProjectTile({ p, now, busy, onResume, onPin, onHide, onOpenDir }: {
  p: Project; now: number; busy: boolean;
  onResume: (mode: ResumeMode) => void; onPin: () => void; onHide: () => void; onOpenDir: () => void;
}) {
  const [menu, setMenu] = useState(false);
  const root = useRef<HTMLElement>(null);
  // Menu : se ferme au clic en dehors et sur Échap.
  useEffect(() => {
    if (!menu) return;
    const out = (e: Event) => { if (!(e.target as Element).closest?.(".proj-menu, .proj-more")) setMenu(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") { setMenu(false); root.current?.querySelector<HTMLElement>(".proj-more")?.focus(); } };
    document.addEventListener("pointerdown", out);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("pointerdown", out); document.removeEventListener("keydown", esc); };
  }, [menu]);

  const h = hueOf(p.name);
  const style = { "--g1": hsl(h, LIGHT), "--g2": hsl((h + 40) % 360, DARK), "--ring": `hsl(${h} 75% 62%)` } as CSSProperties;
  const plan = p.plan && p.plan.total > 0 ? p.plan : null;
  const days = p.activity_14d.filter((c) => c > 0).length;
  const max = Math.max(1, ...p.activity_14d);
  const item = (fn: () => void) => () => { setMenu(false); fn(); };

  return (
    <article ref={root} className="proj" style={style} aria-label={`Projet ${p.name}`}>
      <div className="proj-h">
        <div className="proj-ico" aria-hidden>{badge(p.name)}</div>
        <div className="proj-name"><b title={p.name}>{p.name}</b><small>{ago(p.last_activity_ms, now)}</small></div>
        <div className="proj-ring">
          {plan ? (
            <>
              <svg width="38" height="38" aria-hidden><circle cx="19" cy="19" r="15" className="bg" /><circle cx="19" cy="19" r="15" className="fg" strokeDasharray={C} strokeDashoffset={C * (1 - Math.min(plan.done / plan.total, 1))} /></svg>
              <span role="img" aria-label={`étape ${plan.done} sur ${plan.total}`}>{plan.done}/{plan.total}</span>
            </>
          ) : <span>—</span>}
        </div>
        <button type="button" className="proj-more" aria-label={`Options de ${p.name}`} aria-expanded={menu} onClick={() => setMenu(!menu)}><MoreHorizontal size={14} /></button>
        {menu && (
          <div className="proj-menu">
            <button type="button" autoFocus onClick={item(onPin)}>{p.pinned ? "Désépingler" : "Épingler"}</button>
            <button type="button" onClick={item(onHide)}>Masquer</button>
            <button type="button" onClick={item(onOpenDir)}>Ouvrir le dossier</button>
          </div>
        )}
      </div>
      <div className="proj-last">
        {plan?.current ? <>Tu en étais à : <em>{plan.current}</em></>
          : p.last_session ? <>Dernière session : <em>{p.last_session.title}</em></>
          : "Rien de récent"}
      </div>
      <div className="proj-heat" aria-hidden>
        {p.activity_14d.map((c, i) => <i key={i} style={c > 0 ? { opacity: 0.3 + 0.7 * (c / max), background: "var(--ring)" } : undefined} />)}
      </div>
      <span className="sr-only">activité : {days} jours sur 14</span>
      <div className="proj-chips">
        {p.branch && <span className="chip">⑂ {p.branch}</span>}
        <span className="chip">{p.dirty > 0 ? `${p.dirty} modifs` : "propre"}</span>
      </div>
      <div className="proj-split">
        <button type="button" className="n" disabled={busy} onClick={() => onResume("normal")}>Reprendre</button>
        <button type="button" className="o" disabled={busy} onClick={() => onResume("orchestrator")}>⧉ Orchestr.</button>
      </div>
    </article>
  );
}
