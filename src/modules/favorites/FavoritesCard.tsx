import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from "react";
import { MoreHorizontal, Plus, Star } from "lucide-react";
import { Card } from "../../ui/Card";
import { Empty } from "../../ui/Empty";
import { useTopic } from "../../lib/data";
import { useToolAction } from "../../lib/toolAction";
import { favoritesIpc, type Favorite } from "./api";
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
  let h = 0;
  for (const c of host) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h % 360;
}

const move = <T,>(a: T[], from: number, to: number) => { const b = [...a]; b.splice(to, 0, b.splice(from, 1)[0]); return b; };

type Drag = { id: number; x: number; y: number; active: boolean; timer: number; over: number | null };

export function FavoritesCard() {
  const { data } = useTopic<Favorite[]>("favorites", favoritesIpc.list, NONE);
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

  const persist = (next: Favorite[]) => { setLocal(next); favoritesIpc.reorder(next.map((f) => f.id)).catch(() => setLocal(null)); };

  /** Index de la tuile sous le pointeur, dans la même catégorie que la tuile saisie. */
  const target = (x: number, y: number, from: number) => {
    for (const el of root.current?.querySelectorAll<HTMLElement>("[data-index]") ?? []) {
      const r = el.getBoundingClientRect(), i = Number(el.dataset.index);
      if (x >= r.left && x < r.right && y >= r.top && y < r.bottom) return list[i].category === list[from].category ? i : null;
    }
    return null;
  };

  const start = (d: Drag) => { if (drag.current === d) { d.active = true; setDragging(d.id); } };
  const down = (e: PointerEvent, id: number) => {
    if (e.button !== 0) return;
    const d: Drag = { id, x: e.clientX, y: e.clientY, active: false, over: null, timer: 0 };
    d.timer = window.setTimeout(() => start(d), HOLD_MS);
    drag.current = d;
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
    drag.current = null;
    setDragging(null);
    setOver(null);
    if (!d.active) return;
    justDragged.current = true; // le clic qui suit le relâchement n'ouvre rien
    setTimeout(() => { justDragged.current = false; }, 0);
    const from = list.findIndex((f) => f.id === d.id);
    if (d.over !== null && d.over !== from) persist(move(list, from, d.over));
  };

  const key = (e: KeyboardEvent, i: number) => {
    if (!e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
    e.preventDefault();
    const to = i + (e.key === "ArrowLeft" ? -1 : 1);
    if (to < 0 || to >= list.length || list[to].category !== list[i].category) return;
    persist(move(list, i, to));
  };

  const submit = async () => {
    if (!url.trim()) return;
    if (await run(favoritesIpc.add(url.trim(), title.trim() || undefined))) { setAdding(false); setUrl(""); setTitle(""); }
  };

  const categories = [...new Set(list.map((f) => f.category))];
  const grouped = categories.length > 1;
  const groups = grouped ? categories : [null];
  const open = (f: Favorite) => { if (!justDragged.current) void run(favoritesIpc.open(f.id)); };
  const draggedAt = dragging === null ? -1 : list.findIndex((x) => x.id === dragging);

  const tile = (f: Favorite) => {
    const i = list.indexOf(f);
    const drop = over === i && draggedAt >= 0 ? (i > draggedAt ? "drop-after" : "drop-before") : "";
    return (
      <li key={f.id} data-index={i} className={`fav ${dragging === f.id ? "dragging" : ""} ${drop}`} onContextMenu={(e) => { e.preventDefault(); setMenu(f.id); }}>
        <button type="button" className="fav-btn" aria-label={f.title} onClick={() => open(f)}
          onPointerDown={(e) => down(e, f.id)} onPointerMove={moved} onPointerUp={up} onPointerCancel={up}
          onKeyDown={(e) => key(e, i)}>
          <span className="fav-ico" style={{ "--hue": hueOf(hostOf(f.url)) } as CSSProperties}>
            {f.icon ? <img src={f.icon} alt="" draggable={false} /> : <span aria-hidden>{initials(f.title)}</span>}
          </span>
          {renaming === f.id ? null : <span className="fav-t" aria-hidden>{f.title}</span>}
        </button>
        {renaming === f.id && (
          <input className="fav-rename" aria-label="Nouveau titre" defaultValue={f.title} autoFocus spellCheck={false}
            onBlur={() => setRenaming(null)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setRenaming(null);
              if (e.key === "Enter") { const v = e.currentTarget.value.trim(); setRenaming(null); if (v) favoritesIpc.rename(f.id, v).catch(() => {}); }
            }} />
        )}
        <button type="button" className="fav-more" aria-label={`Options de ${f.title}`} aria-haspopup="menu" onClick={() => setMenu(menu === f.id ? null : f.id)}><MoreHorizontal size={12} /></button>
        {menu === f.id && (
          <div className="fav-menu" role="menu" onKeyDown={(e) => e.key === "Escape" && setMenu(null)}>
            <button type="button" role="menuitem" autoFocus onClick={() => { setMenu(null); setRenaming(f.id); }}>Renommer</button>
            <button type="button" role="menuitem" onClick={() => { setMenu(null); void run(favoritesIpc.remove(f.id)); }}>Supprimer</button>
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
            <button type="button" disabled title="Bientôt">Importer</button>
          </>
        )}
        {groups.map((cat, g) => (
          <div key={cat ?? "∅"}>
            {grouped && <h4 className="fav-cat">{cat ?? "Sans catégorie"}</h4>}
            <ul className="favs">
              {list.filter((f) => !grouped || f.category === cat).map(tile)}
              {g === groups.length - 1 && (
                <li><button type="button" className="fav-btn fav-add" aria-label="Ajouter un favori" onClick={() => { setAdding(!adding); clear(); }}>
                  <span className="fav-ico"><Plus size={16} aria-hidden /></span><span className="fav-t" aria-hidden>Ajouter</span></button></li>
              )}
            </ul>
          </div>
        ))}
      </div>
      {adding && (
        <form className="fav-form" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
          <input aria-label="Adresse du site" placeholder="https://…" value={url} onChange={(e) => setUrl(e.target.value)} autoFocus spellCheck={false} />
          <input aria-label="Titre (facultatif)" placeholder="Titre (facultatif)" value={title} onChange={(e) => setTitle(e.target.value)} spellCheck={false} />
          <button type="submit">Ajouter</button>
        </form>
      )}
      {error && <p className="err" role="alert">{error}</p>}
    </Card>
  );
}
