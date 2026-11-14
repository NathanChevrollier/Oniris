import { memo, useCallback, useEffect, useId, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from "react";
import { MoreHorizontal, Music, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useToolAction } from "../../lib/toolAction";
import { usePanelStore } from "../../panel/store";
import { musicIpc, type Device, type NowPlaying, type PlaylistsView } from "./api";
import "./music.css";

const UNAVAILABLE = "Musique indisponible, réessaie.";
const STEP_MS = 5000;
const MAX_FAVORITES = 8;
const BARS = 28;
const NONE_COLORS: string[] = [];
const NO_DEVICES: Device[] = [];
const HEX = /^#[0-9a-f]{6}$/i;

/** Trois couleurs `#rrggbb` valides, sinon aucune (ambiance par défaut) : rien d'autre n'entre dans le CSS. */
export const safeColors = (colors: string[] | undefined): string[] => {
  const ok = (colors ?? []).filter((c) => typeof c === "string" && HEX.test(c));
  return ok.length >= 3 ? ok.slice(0, 3) : NONE_COLORS;
};

export const clock = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/** Position courante : extrapolée depuis le dernier événement, bornée à la durée. */
export const positionAt = (np: NowPlaying, now: number) =>
  Math.min(np.duration_ms, Math.max(0, np.position_ms + (np.playing ? now - np.updated_ms : 0)));

/** Un seul `requestAnimationFrame`, seulement si le panneau est ouvert, l'onglet visible et la lecture en cours ; arrêté en fin de piste. */
function usePosition(np: NowPlaying, active: boolean): number {
  const [pos, setPos] = useState(() => positionAt(np, Date.now()));
  const [hidden, setHidden] = useState(() => typeof document !== "undefined" && document.hidden);
  useEffect(() => {
    const on = () => setHidden(document.hidden);
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, []);
  useEffect(() => {
    setPos(positionAt(np, Date.now()));
    if (!np.playing || !active || hidden || np.duration_ms <= 0) return;
    let raf = 0;
    let last = -1;
    const tick = () => {
      const p = positionAt(np, Date.now());
      const q = Math.floor(p / 250); // ~4 rendus par seconde, de la barre seule ; elle glisse par transition CSS
      if (q !== last) { last = q; setPos(p); }
      if (p < np.duration_ms) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [np, active, hidden]);
  return pos;
}

/** Dégradé stable par playlist (les pochettes distantes ne sont pas chargées). */
const gradient = (id: string) => {
  let h = 0;
  for (const c of id) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  h = (h * 137.5) % 360; // pas d'or : des ids voisins donnent des teintes éloignées
  return `linear-gradient(135deg,hsl(${h} 80% 60%),hsl(${(h + 60) % 360} 70% 32%))`;
};

function Wave({ playing }: { playing: boolean }) {
  return (
    <div className={`mu-wave${playing ? "" : " paused"}`} data-testid="wave" aria-hidden>
      {Array.from({ length: BARS }, (_, i) => (
        <i key={i} style={{ "--h": 0.25 + ((i * 37) % 10) / 14, animationDelay: `${-((i * 137) % 900)}ms`, animationDuration: `${700 + ((i * 53) % 5) * 110}ms` } as CSSProperties} />
      ))}
    </div>
  );
}

/** Barre de progression : porte seule l'état de position, le reste de la carte ne se redessine pas. */
const Progress = memo(function Progress({ np, active, onSeek }: { np: NowPlaying; active: boolean; onSeek: (ms: number) => void }) {
  const pos = usePosition(np, active);
  const ref = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const known = np.duration_ms > 0; // durée inconnue : la barre ne déplace rien
  const at = (e: PointerEvent) => {
    const r = ref.current!.getBoundingClientRect();
    return r.width > 0 ? Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * np.duration_ms : 0;
  };
  const shown = drag ?? pos;
  const key = (e: KeyboardEvent) => {
    if (!known) return;
    const target = e.key === "ArrowRight" ? pos + STEP_MS : e.key === "ArrowLeft" ? pos - STEP_MS : e.key === "Home" ? 0 : e.key === "End" ? np.duration_ms : null;
    if (target == null) return;
    e.preventDefault();
    onSeek(Math.min(np.duration_ms, Math.max(0, target)));
  };
  return (
    <>
      <div ref={ref} className="mu-scrub" role="slider" tabIndex={0} aria-label="Position" aria-valuemin={0} aria-valuemax={np.duration_ms}
        aria-valuenow={Math.round(shown)} aria-valuetext={known ? `${clock(shown)} sur ${clock(np.duration_ms)}` : "Durée inconnue"}
        aria-disabled={known ? undefined : true} onKeyDown={key}
        onPointerDown={(e) => { if (!known || e.button !== 0) return; ref.current?.setPointerCapture?.(e.pointerId); setDrag(at(e)); }}
        onPointerMove={(e) => { if (drag != null) setDrag(at(e)); }}
        onPointerUp={(e) => { if (drag != null) { onSeek(at(e)); setDrag(null); } }}
        onPointerCancel={() => setDrag(null)}>
        <i className={drag != null ? "drag" : undefined} style={{ width: `${known ? (shown / np.duration_ms) * 100 : 0}%` }} />
      </div>
      <div className="mu-times"><span>{clock(shown)}</span><span>{known ? `-${clock(np.duration_ms - shown)}` : ""}</span></div>
    </>
  );
});

export function MusicCard({ onAccounts, onColors }: { onAccounts?: () => void; onColors?: (colors: string[]) => void }) {
  const open = usePanelStore((s) => s.open);
  const now = useTopic<NowPlaying | null>("music", musicIpc.now, null);
  const { run, error } = useToolAction(UNAVAILABLE);
  const [menu, setMenu] = useState(false);
  const menuId = useId();
  const footRef = useRef<HTMLDivElement>(null);
  const [pinErr, setPinErr] = useState<string | null>(null);
  // Playlist en cours de lancement : Spotify fermé, l'ouvrir prend quelques secondes.
  const [starting, setStarting] = useState<string | null>(null);
  const playList = async (id: string) => {
    setStarting(id);
    try { await run(musicIpc.playPlaylist(id)); } finally { setStarting(null); }
  };
  // Après un déplacement réussi, la position repart de là jusqu'au prochain événement du lecteur.
  const [seeked, setSeeked] = useState<{ base: NowPlaying; ms: number; at: number } | null>(null);
  const np = useMemo(
    () => (now.data && seeked?.base === now.data ? { ...now.data, position_ms: seeked.ms, updated_ms: seeked.at } : now.data),
    [now.data, seeked],
  );

  // Tableau stable tant que les couleurs ne changent pas (la clé les résume).
  const colorKey = JSON.stringify(safeColors(now.data?.colors));
  const colors = useMemo<string[]>(() => JSON.parse(colorKey), [colorKey]);
  useEffect(() => { onColors?.(colors); }, [colors, onColors]);

  // Playlists (cache seul côté Rust) : rechargées à l'ouverture, au changement de piste, ou tant qu'elles ne sont pas là.
  // Appareils (appel Spotify) : seulement connecté, au changement de piste, à l'ouverture et au focus du sélecteur.
  const [view, setViewState] = useState<PlaylistsView | null>(null);
  const viewRef = useRef<PlaylistsView | null>(null);
  const setView = useCallback((v: PlaylistsView | null) => { viewRef.current = v; setViewState(v); }, []);
  const reqLists = useRef(0);
  const loadLists = useCallback(() => {
    const id = ++reqLists.current;
    musicIpc.playlists().then((v) => { if (reqLists.current === id) setView(v); }, () => {});
  }, [setView]);
  const [devices, setDevices] = useState<Device[]>(NO_DEVICES);
  const reqDev = useRef(0);
  const loadDevices = useCallback(() => {
    const id = ++reqDev.current;
    musicIpc.devices().then((d) => { if (reqDev.current === id) setDevices(d); }, () => {});
  }, []);

  const trackKey = now.data ? `${now.data.title}|${now.data.artist}` : "";
  const lastTrack = useRef("");
  useEffect(() => { if (open) loadLists(); }, [open, loadLists]);
  useEffect(() => {
    if (!open) return;
    const v = viewRef.current;
    if (v && v.connected && v.all.length > 0 && lastTrack.current === trackKey) return;
    lastTrack.current = trackKey;
    loadLists();
  }, [open, now.data, trackKey, loadLists]);
  const connected = view?.connected ?? false;
  useEffect(() => { if (open && connected) loadDevices(); }, [open, connected, trackKey, loadDevices]);

  const seek = useCallback(async (ms: number) => {
    if (!now.data || now.data.duration_ms <= 0) return;
    const base = now.data;
    if (await run(musicIpc.seek(ms))) setSeeked({ base, ms, at: Date.now() });
  }, [now.data, run]);

  const togglePin = (id: string) => {
    const cur = viewRef.current;
    if (!cur) return;
    const pinned = cur.favorites.some((p) => p.id === id);
    if (!pinned && cur.favorites.length >= MAX_FAVORITES) { setPinErr(`${MAX_FAVORITES} playlists au maximum.`); return; }
    const added = cur.all.find((p) => p.id === id);
    const favorites = pinned ? cur.favorites.filter((p) => p.id !== id) : added ? [...cur.favorites, added] : cur.favorites;
    setView({ ...cur, favorites }); // optimiste
    setPinErr(null);
    musicIpc.setFavorites(favorites.map((p) => p.id)).catch(() => { setPinErr("Impossible d'enregistrer les playlists."); loadLists(); });
  };

  // Le menu se ferme avec Échap ou un clic à l'extérieur.
  useEffect(() => {
    if (!menu) return;
    const esc = (e: globalThis.KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); setMenu(false); } };
    const out = (e: globalThis.PointerEvent) => { if (!footRef.current?.contains(e.target as Node)) setMenu(false); };
    document.addEventListener("keydown", esc, true);
    document.addEventListener("pointerdown", out);
    return () => { document.removeEventListener("keydown", esc, true); document.removeEventListener("pointerdown", out); };
  }, [menu]);

  const active = devices.find((d) => d.active);

  return (
    <Card title="En lecture" span={4} delay={0.05}
      aside={np ? <span className="mu-src"><i className={np.source === "spotify" ? "sp" : undefined} />{np.source === "spotify" ? "Spotify" : "Lecteur Windows"}</span> : undefined}>
      <div className="mu">
        {np ? (
          <>
            <div className="mu-top">
              <div className="mu-cover" style={{ "--c1": colors[0] ?? "#ff7448", "--c2": colors[1] ?? "#3b1d6e" } as CSSProperties}>
                {np.cover && <img src={np.cover} alt="" />}
              </div>
              <div className="mu-meta">
                <div>
                  <b className="mu-title">{np.title}</b>
                  <div className="mu-artist">{[np.artist, np.album].filter(Boolean).join(" · ")}</div>
                  <span className="sr-only">{np.title}, {np.artist}</span>
                </div>
                <Wave playing={np.playing} />
              </div>
            </div>
            <Progress np={np} active={open} onSeek={seek} />
            <div className="mu-ctrls">
              <button type="button" aria-label="Précédent" onClick={() => void run(musicIpc.previous())}><SkipBack size={20} fill="currentColor" /></button>
              <button type="button" className="pp" aria-label={np.playing ? "Pause" : "Lecture"} onClick={() => void run(musicIpc.toggle())}>
                {np.playing ? <Pause size={20} fill="currentColor" /> : <Play size={20} fill="currentColor" />}
              </button>
              <button type="button" aria-label="Suivant" onClick={() => void run(musicIpc.next())}><SkipForward size={20} fill="currentColor" /></button>
            </div>
          </>
        ) : (
          <div className="mu-idle" role="status">
            <span className="mu-idle-ico"><Music size={22} /></span>
            <div>
              <b>Rien en lecture</b>
              <small>{starting ? "Ouverture de Spotify…" : connected ? "Choisis une playlist : Spotify s'ouvre tout seul s'il est fermé." : "Lance ta musique dans Spotify ou un autre lecteur."}</small>
            </div>
          </div>
        )}
        {(error || pinErr) && <p className="mu-err" role="alert">{error ?? pinErr}</p>}

        {view == null ? (
          <p className="mu-note" role="status">Chargement des playlists…</p>
        ) : !connected ? (
          <button type="button" className="mu-connect" onClick={onAccounts}>Connecte Spotify pour les playlists</button>
        ) : (
          <>
            {view.favorites.length === 0
              ? <p className="mu-note" role="status">{view.all.length === 0 ? "Playlists en cours de chargement…" : "Épingle des playlists avec « … »."}</p>
              : (
                <ul className="mu-lists">
                  {view.favorites.map((p) => (
                    <li key={p.id}>
                      <button type="button" className="mu-pl" aria-label={p.name} aria-busy={starting === p.id} disabled={starting !== null}
                        onClick={() => void playList(p.id)}>
                        <span className="mu-pl-cover" style={{ background: gradient(p.id) }}>
                          {p.image?.startsWith("https://") && <img src={p.image} alt="" loading="lazy" referrerPolicy="no-referrer" />}
                          <i className="mu-pl-play">{starting === p.id ? <span className="mu-spin" /> : <Play size={14} fill="currentColor" />}</i>
                        </span>
                        <span className="mu-pl-name">{p.name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            <div ref={footRef} className="mu-bottom">
              <div className="mu-foot">
                {devices.length > 0 && (
                  <select aria-label="Appareil" value={active?.id ?? ""} onFocus={loadDevices}
                    onChange={(e) => void run(musicIpc.transfer(e.target.value)).then(loadDevices)}>
                    {!active && <option value="" disabled>Appareil…</option>}
                    {devices.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
                  </select>
                )}
                <button type="button" className="mu-more" aria-label="Choisir les playlists" aria-expanded={menu} aria-controls={menuId} onClick={() => setMenu(!menu)}><MoreHorizontal size={16} /></button>
              </div>
              {menu && (
                <ul id={menuId} className="mu-menu" aria-label="Playlists épinglées">
                  {view.all.map((p) => (
                    <li key={p.id}>
                      <label><input type="checkbox" checked={view.favorites.some((f) => f.id === p.id)} onChange={() => togglePin(p.id)} />{p.name}</label>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </>
        )}
      </div>
    </Card>
  );
}

/** Ambiance du panneau : calques de couleurs de la pochette, en fondu de 1,2 s (opacité seule), sur l'ambiance par défaut. */
export function Ambient({ colors }: { colors: string[] }) {
  const [layers, setLayers] = useState<{ id: number; colors: string[]; out: boolean }[]>([]);
  const seq = useRef(0);
  const key = JSON.stringify(safeColors(colors));
  const safe = useMemo<string[]>(() => JSON.parse(key), [key]);
  useEffect(() => {
    const id = ++seq.current;
    if (id === 1 && safe.length < 3) return; // rien à fondre au premier rendu : aucune minuterie
    setLayers((ls) => [...ls.map((l) => ({ ...l, out: true })), ...(safe.length >= 3 ? [{ id, colors: safe, out: false }] : [])]);
    const t = setTimeout(() => setLayers((ls) => ls.filter((l) => !l.out)), 1300);
    return () => clearTimeout(t);
  }, [safe]);
  return (
    <>
      <div className="ambient"><i /><i /><i /></div>
      {layers.map((l) => (
        <div key={l.id} className={`ambient-tint${l.out ? " out" : ""}`} data-testid="ambient-tint"
          style={{ "--ambient-1": l.colors[0], "--ambient-2": l.colors[1], "--ambient-3": l.colors[2] } as CSSProperties} />
      ))}
    </>
  );
}
