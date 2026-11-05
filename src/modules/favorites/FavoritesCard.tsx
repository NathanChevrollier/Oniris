import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from "react";
import { MoreHorizontal, Plus, Star } from "lucide-react";
import { Card } from "../../ui/Card";
import { Empty } from "../../ui/Empty";
import { useTopic } from "../../lib/data";
import { useToolAction } from "../../lib/toolAction";
import { favoritesIpc, type Favorite } from "./api";
import { DARK, LIGHT, hsl } from "./contrast";
import { ImportMenu } from "./ImportMenu";
import "./favorites.css";

const UNAVAILABLE = "Favoris indisponibles, réessaie.";
const HOLD_MS = 150;
const MOVE_PX = 4;
const NONE: Favorite[] = [];

/** Première lettre des deux premiers mots. */
export const initials = (title: string) => title.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => [...w][0]).join("").toUpperCase() || "?";

export const hostOf = (url: string) => { try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return url; } };

/** Teinte stable (0-359) tirée du domaine. */
export function hueOf(host: string) {
  let h = 2166136261; // FNV-1a, puis mélange final : les domaines proches s'étalent sur tout le cercle
  for (const c of host) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0;
  h = Math.imul(h ^ (h >>> 15), 2246822519) >>> 0;
  return ((h ^ (h >>> 13)) >>> 0) % 360;
}

const iconStyle = (h: number) => ({ "--g1": hsl(h, LIGHT), "--g2": hsl((h + 40) % 360, DARK) }) as CSSProperties;

const move = <T,>(a: T[], from: number, to: number) => { const b = [...a]; b.splice(to, 0, b.splice(from, 1)[0]); return b; };

type Drag = { id: number; x: number; y: number; active: boolean; timer: number; over: number | null; el: HTMLElement; pid: number };

export function FavoritesCard() {
  const { data, reload } = useTopic<Favorite[]>("favorites", favoritesIpc.list, NONE);
  const [local, setLocal] = useState<Favorite[] | null>(null); // ordre optimiste jusqu'au prochain rechargement
  useEffect(() => setLocal(null), [data]);
  const list = local ?? data;
  const { run, error, clear } = useToolAction(UNAVAILABLE);
  const [adding, setAdding] = useState(false);
  const [url, setUrl] = useState("");
  const [title, setTitle] = useState("");
  const [menu, setMenu] = useState<number | null>(null);
  const [renaming, setRenaming] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  const drag = useRef<Drag | null>(null);
  const justDragged = useRef(false);
  const root = useRef<HTMLDivElement>(null);
  const upRef = useRef<() => void>(() => {});
  const onWinUp = useRef(() => upRef.current()).current; // identité stable : removeEventListener retire bien ce qui a été ajouté
  const [announce, setAnnounce] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [reorderError, setReorderError] = useState<string | null>(null);
  const fresh = () => { setRenameError(null); setReorderError(null); clear(); };
  const refocusMore = (id: number) => root.current?.querySelector<HTMLElement>(`[data-id="${id}"] .fav-more`)?.focus();

  const persist = (next: Favorite[]) => {
    fresh();
    setLocal(next);
    favoritesIpc.reorder(next.map((f) => f.id)).catch((e) => {
      setLocal(null);
      setReorderError(String(e).includes("ordre incomplet ou inconnu") ? "Liste modifiée ailleurs — réessaie" : "Réorganisation impossible");
      reload();
    });
  };

  /** Index de la tuile sous le pointeur, dans la même catégorie que la tuile saisie. */
  const target = (x: number, y: number, from: number) => {
    for (const el of root.current?.querySelectorAll<HTMLElement>("[data-index]") ?? []) {
      const r = el.getBoundingClientRect(), i = Number(el.dataset.index);
      if (x >= r.left && x < r.right && y >= r.top && y < r.bottom) return list[i].category === list[from].category ? i : null;
    }
    return null;
  };

  const start = (d: Drag) => {
    if (drag.current !== d || d.active) return;
    d.active = true;
    setDragging(d.id);
    try { d.el.setPointerCapture?.(d.pid); } catch { /* pointeur déjà relâché */ }
  };
  // Le relâchement peut avoir lieu hors de la tuile (ou de la fenêtre) : on l'écoute sur la fenêtre.
  const down = (e: PointerEvent, id: number) => {
    if (e.button !== 0) return;
    const d: Drag = { id, x: e.clientX, y: e.clientY, active: false, over: null, timer: 0, el: e.currentTarget as HTMLElement, pid: e.pointerId };
    d.timer = window.setTimeout(() => start(d), HOLD_MS);
    drag.current = d;
    window.addEventListener("pointerup", onWinUp);
    window.addEventListener("pointercancel", onWinUp);
  };
  const moved = (e: PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    if (!d.active && Math.hypot(e.clientX - d.x, e.clientY - d.y) >= MOVE_PX) start(d);
    if (!d.active) return;
    d.over = target(e.clientX, e.clientY, list.findIndex((f) => f.id === d.id));
    setOver(d.over);
  };
  const up = () => {
    const d = drag.current;
    if (!d) return;
    clearTimeout(d.timer);
    window.removeEventListener("pointerup", onWinUp);
    window.removeEventListener("pointercancel", onWinUp);
    try { d.el.releasePointerCapture?.(d.pid); } catch { /* déjà libéré */ }
    drag.current = null;
    setDragging(null);
    setOver(null);
    if (!d.active) return;
    justDragged.current = true; // le clic qui suit le relâchement n'ouvre rien
    setTimeout(() => { justDragged.current = false; }, 0);
    const from = list.findIndex((f) => f.id === d.id);
    if (d.over !== null && d.over !== from) persist(move(list, from, d.over));
  };

  upRef.current = up;
  useEffect(() => () => { clearTimeout(drag.current?.timer); window.removeEventListener("pointerup", onWinUp); window.removeEventListener("pointercancel", onWinUp); }, [onWinUp]);
  // Menu : se ferme au clic en dehors et sur Échap.
  useEffect(() => {
    if (menu === null) return;
    const out = (e: Event) => { if (!(e.target as Element).closest?.(".fav-menu, .fav-more")) setMenu(null); };
    const esc = (e: globalThis.KeyboardEvent) => { if (e.key === "Escape") { setMenu(null); refocusMore(menu); } };
    document.addEventListener("pointerdown", out);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("pointerdown", out); document.removeEventListener("keydown", esc); };
  }, [menu]);

  const key = (e: KeyboardEvent, i: number) => {
    if (!e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
    e.preventDefault();
    const to = i + (e.key === "ArrowLeft" ? -1 : 1);
    if (to < 0 || to >= list.length || list[to].category !== list[i].category) return;
    persist(move(list, i, to));
    setAnnounce(`Déplacé en position ${to + 1} sur ${list.length}`);
  };

  const submit = async () => {
    if (!url.trim()) return;
    fresh();
    if (await run(favoritesIpc.add(url.trim(), title.trim() || undefined))) { setAdding(false); setUrl(""); setTitle(""); }
  };

  const categories = [...new Set(list.map((f) => f.category))];
  const grouped = categories.length > 1;
  const groups = grouped ? categories : [null];
  const open = (f: Favorite) => {
    if (justDragged.current) return;
    fresh();
    void run(favoritesIpc.open(f.id));
  };
  /** Supprime puis place le focus sur la tuile suivante, à défaut la précédente, à défaut « + » ("…" a disparu avec la tuile). */
  const remove = async (f: Favorite) => {
    const i = list.indexOf(f);
    const next = list[i + 1] ?? list[i - 1];
    fresh();
    const ok = await run(favoritesIpc.remove(f.id));
    const sel = !ok ? `[data-id="${f.id}"] .fav-more` : next ? `[data-id="${next.id}"] .fav-btn` : ".fav-add";
    root.current?.querySelector<HTMLElement>(sel)?.focus();
  };
  const draggedAt = dragging === null ? -1 : list.findIndex((x) => x.id === dragging);

  const tile = (f: Favorite) => {
    const i = list.indexOf(f);
    const drop = over === i && draggedAt >= 0 ? (i > draggedAt ? "drop-after" : "drop-before") : "";
    return (
      <li key={f.id} data-index={i} data-id={f.id} className={`fav ${dragging === f.id ? "dragging" : ""} ${drop}`} onContextMenu={(e) => { e.preventDefault(); setMenu(f.id); }}>
        <button type="button" className="fav-btn" aria-label={f.title} onClick={() => open(f)}
          onPointerDown={(e) => down(e, f.id)} onPointerMove={moved} onLostPointerCapture={up}
          aria-keyshortcuts="Alt+ArrowLeft Alt+ArrowRight" onKeyDown={(e) => key(e, i)}>
          <span className="fav-ico" style={iconStyle(hueOf(hostOf(f.url)))}>
            {f.icon ? <img src={f.icon} alt="" draggable={false} /> : <span aria-hidden>{initials(f.title)}</span>}
          </span>
          {renaming === f.id ? null : <span className="fav-t" aria-hidden>{f.title}</span>}
        </button>
        {renaming === f.id && (
          <input className="fav-rename" aria-label="Nouveau titre" defaultValue={f.title} autoFocus spellCheck={false}
            onBlur={() => setRenaming(null)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setRenaming(null);
              if (e.key === "Enter") { const v = e.currentTarget.value.trim(); setRenaming(null); if (v) fresh(), favoritesIpc.rename(f.id, v).then(() => setRenameError(null), () => setRenameError("Renommage impossible, réessaie.")); }
            }} />
        )}
        <button type="button" className="fav-more" aria-label={`Options de ${f.title}`} aria-haspopup="menu" onClick={() => { fresh(); setMenu(menu === f.id ? null : f.id); }}><MoreHorizontal size={12} /></button>
        {menu === f.id && (
          <div className="fav-menu" role="menu">
            <button type="button" role="menuitem" autoFocus onClick={() => { fresh(); setMenu(null); setRenaming(f.id); }}>Renommer</button>
            <button type="button" role="menuitem" onClick={() => { setMenu(null); void remove(f); }}>Supprimer</button>
          </div>
        )}
      </li>
    );
  };

  return (
    <Card title="Favoris" span={3} delay={0.3}>
      <div ref={root}>
        {list.length === 0 && (
          <>
            <Empty icon={Star} tint="#bf5af2">Ajoute tes sites favoris, ou importe-les</Empty>
          </>
        )}
        {groups.map((cat, g) => (
          <div key={cat ?? "∅"}>
            {grouped && <h4 className="fav-cat">{cat ?? "Sans catégorie"}</h4>}
            <ul className="favs" role="list" aria-label={grouped ? `Favoris — ${cat ?? "Sans catégorie"}` : "Favoris"}>
              {list.filter((f) => !grouped || f.category === cat).map(tile)}
              {g === groups.length - 1 && (
                <li><button type="button" className="fav-btn fav-add" aria-label="Ajouter un favori" onClick={() => { fresh(); setAdding(!adding); }}>
                  <span className="fav-ico"><Plus size={16} aria-hidden /></span><span className="fav-t" aria-hidden>Ajouter</span></button></li>
              )}
            </ul>
          </div>
        ))}
      </div>
      <ImportMenu />
      {adding && (
        <form className="fav-form" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
          <input aria-label="Adresse du site" placeholder="https://…" value={url} onChange={(e) => setUrl(e.target.value)} autoFocus spellCheck={false} />
          <input aria-label="Titre (facultatif)" placeholder="Titre (facultatif)" value={title} onChange={(e) => setTitle(e.target.value)} spellCheck={false} />
          <button type="submit">Ajouter</button>
        </form>
      )}
      {[error, renameError, reorderError].map((m) => m && <p key={m} className="err" role="alert">{m}</p>)}
      <span className="sr-only" role="status" aria-live="polite">{announce}</span>
    </Card>
  );
}
