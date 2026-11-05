import { useCallback, useEffect, useId, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from "react";
import { MoreHorizontal, Music, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import { Card } from "../../ui/Card";
import { Empty } from "../../ui/Empty";
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

export const clock = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/** Position courante : extrapolée depuis le dernier événement, bornée à la durée. */
export const positionAt = (np: NowPlaying, now: number) =>
  Math.min(np.duration_ms, Math.max(0, np.position_ms + (np.playing ? now - np.updated_ms : 0)));

/** Un seul `requestAnimationFrame`, seulement si le panneau est ouvert, l'onglet visible et la lecture en cours. */
function usePosition(np: NowPlaying | null, active: boolean): number {
  const [pos, setPos] = useState(0);
  const [hidden, setHidden] = useState(() => typeof document !== "undefined" && document.hidden);
  useEffect(() => {
    const on = () => setHidden(document.hidden);
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, []);
  useEffect(() => {
    if (!np) return;
    setPos(positionAt(np, Date.now()));
    if (!np.playing || !active || hidden) return;
    let raf = 0;
    let last = -1;
    const tick = () => {
      const p = positionAt(np, Date.now());
      const q = Math.floor(p / 250); // ~4 rendus par seconde, la barre glisse par transition CSS
      if (q !== last) { last = q; setPos(p); }
      raf = requestAnimationFrame(tick);
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
        <i key={i} style={{ animationDelay: `${-((i * 137) % 900)}ms`, animationDuration: `${700 + ((i * 53) % 5) * 110}ms` }} />
      ))}
    </div>
  );
}

function Progress({ np, pos, onSeek }: { np: NowPlaying; pos: number; onSeek: (ms: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const at = (e: PointerEvent) => {
    const r = ref.current!.getBoundingClientRect();
    return r.width > 0 ? Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * np.duration_ms : 0;
  };
  const shown = drag ?? pos;
  const key = (e: KeyboardEvent) => {
    const d = e.key === "ArrowRight" ? STEP_MS : e.key === "ArrowLeft" ? -STEP_MS : 0;
    if (!d) return;
    e.preventDefault();
    onSeek(Math.min(np.duration_ms, Math.max(0, pos + d)));
  };
  return (
    <>
      <div ref={ref} className="mu-scrub" role="slider" tabIndex={0} aria-label="Position" aria-valuemin={0} aria-valuemax={np.duration_ms}
        aria-valuenow={Math.round(shown)} aria-valuetext={`${clock(shown)} sur ${clock(np.duration_ms)}`} onKeyDown={key}
        onPointerDown={(e) => { ref.current?.setPointerCapture?.(e.pointerId); setDrag(at(e)); }}
        onPointerMove={(e) => { if (drag != null) setDrag(at(e)); }}
        onPointerUp={(e) => { if (drag != null) { onSeek(at(e)); setDrag(null); } }}
        onPointerCancel={() => setDrag(null)}>
        <i className={drag != null ? "drag" : undefined} style={{ width: `${np.duration_ms ? (shown / np.duration_ms) * 100 : 0}%` }} />
      </div>
      <div className="mu-times"><span>{clock(shown)}</span><span>-{clock(np.duration_ms - shown)}</span></div>
    </>
  );
}

export function MusicCard({ onAccounts, onColors }: { onAccounts?: () => void; onColors?: (colors: string[]) => void }) {
  const open = usePanelStore((s) => s.open);
  const now = useTopic<NowPlaying | null>("music", musicIpc.now, null);
  const lists = useTopic<PlaylistsView | null>("music", musicIpc.playlists, null);
  const devices = useTopic<Device[]>("music", () => musicIpc.devices().catch(() => NO_DEVICES), NO_DEVICES);
  const { run, error } = useToolAction(UNAVAILABLE);
  const [menu, setMenu] = useState(false);
  const menuId = useId();
  // Après un déplacement, la position repart de là jusqu'au prochain événement du lecteur.
  const [seeked, setSeeked] = useState<{ base: NowPlaying; ms: number; at: number } | null>(null);
  const np = useMemo(
    () => (now.data && seeked?.base === now.data ? { ...now.data, position_ms: seeked.ms, updated_ms: seeked.at } : now.data),
    [now.data, seeked],
  );
  const pos = usePosition(np, open);

  const colors = now.data?.colors ?? NONE_COLORS;
  const colorKey = colors.join();
  useEffect(() => { onColors?.(colors); }, [colorKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const seek = useCallback((ms: number) => {
    if (!now.data) return;
    setSeeked({ base: now.data, ms, at: Date.now() });
    void run(musicIpc.seek(ms));
  }, [now.data, run]);

  const view = lists.data;
  const connected = view?.connected ?? false;
  const active = devices.data.find((d) => d.active);
  const togglePin = (id: string) => {
    if (!view) return;
    const ids = view.favorites.map((p) => p.id);
    const next = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id].slice(0, MAX_FAVORITES);
    void musicIpc.setFavorites(next).then(lists.reload, lists.reload);
  };

  return (
    <Card title="En lecture" span={4} delay={0.05}
      aside={np ? <span className="mu-src"><i className={np.source === "spotify" ? "sp" : undefined} />{np.source === "spotify" ? "Spotify" : "Lecteur Windows"}</span> : undefined}>
      <div className="mu">
        {np ? (
          <>
            <div className="mu-top">
              <div className="mu-cover" style={{ "--c1": np.colors[0] ?? "#ff7448", "--c2": np.colors[1] ?? "#3b1d6e" } as CSSProperties}>
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
            <Progress np={np} pos={pos} onSeek={seek} />
            <div className="mu-ctrls">
              <button type="button" aria-label="Précédent" onClick={() => void run(musicIpc.previous())}><SkipBack size={20} fill="currentColor" /></button>
              <button type="button" className="pp" aria-label={np.playing ? "Pause" : "Lecture"} onClick={() => void run(musicIpc.toggle())}>
                {np.playing ? <Pause size={20} fill="currentColor" /> : <Play size={20} fill="currentColor" />}
              </button>
              <button type="button" aria-label="Suivant" onClick={() => void run(musicIpc.next())}><SkipForward size={20} fill="currentColor" /></button>
            </div>
          </>
        ) : (
          <Empty icon={Music} tint="#ff7448" role="status">Rien en lecture</Empty>
        )}
        {error && <p className="mu-err" role="alert">{error}</p>}

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
                      <button type="button" className="mu-pl" onClick={() => void run(musicIpc.playPlaylist(p.id))}>
                        <span style={{ background: gradient(p.id) }} />{p.name}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            <div className="mu-foot">
              {devices.data.length > 0 && (
                <select aria-label="Appareil" value={active?.id ?? ""} onChange={(e) => void run(musicIpc.transfer(e.target.value)).then(devices.reload)}>
                  {!active && <option value="" disabled>Appareil…</option>}
                  {devices.data.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
                </select>
              )}
              <button type="button" className="mu-more" aria-label="Choisir les playlists" aria-expanded={menu} aria-controls={menuId} onClick={() => setMenu(!menu)}><MoreHorizontal size={16} /></button>
            </div>
            {menu && (
              <ul id={menuId} className="mu-menu" aria-label="Playlists épinglées">
                {view.all.map((p) => {
                  const pinned = view.favorites.some((f) => f.id === p.id);
                  return (
                    <li key={p.id}>
                      <label><input type="checkbox" checked={pinned} disabled={!pinned && view.favorites.length >= MAX_FAVORITES} onChange={() => togglePin(p.id)} />{p.name}</label>
                    </li>
                  );
                })}
              </ul>
            )}
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
  const key = colors.slice(0, 3).join();
  useEffect(() => {
    const id = ++seq.current;
    if (id === 1 && colors.length < 3) return; // rien à fondre au premier rendu : aucune minuterie
    setLayers((ls) => [...ls.map((l) => ({ ...l, out: true })), ...(colors.length >= 3 ? [{ id, colors: colors.slice(0, 3), out: false }] : [])]);
    const t = setTimeout(() => setLayers((ls) => ls.filter((l) => !l.out)), 1300);
    return () => clearTimeout(t);
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
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
