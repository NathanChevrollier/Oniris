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
  // Cases cochées / cases du dernier plan de docs/superpowers/plans ; masqué sans case à cocher.
  const plan = p.plan && p.plan.total > 0 ? p.plan : null;
  const days = p.activity_14d.filter((c) => c > 0).length;
  const max = Math.max(1, ...p.activity_14d);
  const item = (fn: () => void) => () => { setMenu(false); fn(); };

  return (
    <article ref={root} className="proj" style={style} aria-label={`Projet ${p.name}`}>
      <div className="proj-h">
        <div className="proj-ico" aria-hidden>{badge(p.name)}</div>
        <div className="proj-name">
          <b title={p.name}>{p.name}</b>
          <small>
            {ago(p.last_activity_ms, now)}
            {p.branch && <span className="proj-br" title={`Branche ${p.branch}`}> · ⑂ {p.branch}</span>}
            <span className={p.dirty > 0 ? "proj-dirty" : undefined}> · {p.dirty > 0 ? `${p.dirty} modifs` : "propre"}</span>
          </small>
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
      <div className="proj-last" title={plan?.current ? `Tu en étais à : ${plan.current}` : p.last_session ? `Dernière session : ${p.last_session.title}` : undefined}>
        {plan?.current ? <>Tu en étais à : <em>{plan.current}</em></>
          : p.last_session ? <>Dernière session : <em>{p.last_session.title}</em></>
          : "Rien de récent"}
      </div>
      {plan && (
        <div className="proj-plan" title={`Cases cochées dans le plan ${plan.file} (docs/superpowers/plans) : ${plan.done} sur ${plan.total}`}>
          <span>Plan : {plan.done}/{plan.total} tâches</span>
          <i role="progressbar" aria-label={`Plan ${plan.file}`} aria-valuemin={0} aria-valuemax={plan.total} aria-valuenow={plan.done}
            aria-valuetext={`${plan.done} tâches cochées sur ${plan.total}`}>
            <b style={{ transform: `scaleX(${Math.min(plan.done / plan.total, 1)})` }} />
          </i>
        </div>
      )}
      <div className="proj-heat" aria-hidden>
        {p.activity_14d.map((c, i) => <i key={i} style={c > 0 ? { opacity: 0.3 + 0.7 * (c / max), background: "var(--ring)" } : undefined} />)}
      </div>
      <span className="sr-only">activité : {days} jours sur 14</span>
      <div className="proj-split">
        <button type="button" className="n" disabled={busy} onClick={() => onResume("normal")}>Reprendre</button>
        <button type="button" className="o" disabled={busy} onClick={() => onResume("orchestrator")}>⧉ Orchestr.</button>
      </div>
    </article>
  );
}
