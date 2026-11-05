import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

/** Miroir de `oniris_core::music::NowPlaying`. */
export type NowPlaying = {
  title: string; artist: string; album: string;
  /** `data:image/png;base64,…` (300 px au plus). */
  cover: string | null;
  /** 3 couleurs dominantes `#rrggbb`. */
  colors: string[];
  playing: boolean; position_ms: number; duration_ms: number;
  /** Instant (ms Unix) de `position_ms` : la position est extrapolée pendant la lecture. */
  updated_ms: number;
  source: "spotify" | "other";
};
export type Playlist = { id: string; name: string; image: string | null; owner_is_me: boolean };
export type Device = { id: string; name: string; kind: string; active: boolean; volume: number | null };
/** Miroir de `music::tools::PlaylistsView`. */
export type PlaylistsView = { connected: boolean; favorites: Playlist[]; all: Playlist[] };

const FAKE_COLORS = ["#ff7448", "#ff3d7f", "#7b4dff"];
// Pochette factice (dégradé SVG) pour l'aperçu navigateur.
const FAKE_COVER = "data:image/svg+xml;utf8," + encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${FAKE_COLORS[0]}"/><stop offset=".5" stop-color="${FAKE_COLORS[1]}"/><stop offset="1" stop-color="#3b1d6e"/></linearGradient></defs><rect width="100" height="100" fill="url(#g)"/><circle cx="70" cy="28" r="11" fill="#ffd27a"/><ellipse cx="50" cy="104" rx="64" ry="32" fill="#1a0f2e"/></svg>`);

const fakeNow = (): NowPlaying => ({
  title: "Nuit Blanche", artist: "Polo & Pan", album: "Focus Deep", cover: FAKE_COVER, colors: FAKE_COLORS, playing: true,
  position_ms: 102_000, duration_ms: 245_000, updated_ms: Date.now(), source: "spotify",
});
const pl = (id: string, name: string): Playlist => ({ id, name, image: null, owner_is_me: true });
const FAKE_LISTS = [pl("p1", "Focus Deep"), pl("p2", "Chill"), pl("p3", "Sport"), pl("p4", "Découvertes"), pl("p5", "Road trip"), pl("p6", "Jazz du soir")];
const FAKE_DEVICES: Device[] = [
  { id: "d1", name: "Ce PC", kind: "Computer", active: true, volume: 60 },
  { id: "d2", name: "Enceinte salon", kind: "Speaker", active: false, volume: 35 },
];
const done: ToolOutcome = { Done: { action_id: 0, output: {} } };
const call = (cmd: string, args?: Record<string, unknown>): Promise<ToolOutcome> => isTauri ? invoke(cmd, args) : Promise.resolve(done);

export const musicIpc = {
  now: (): Promise<NowPlaying | null> => isTauri ? invoke("music_now") : Promise.resolve(fakeNow()),
  toggle: () => call("music_toggle"),
  next: () => call("music_next"),
  previous: () => call("music_previous"),
  seek: (positionMs: number) => call("music_seek", { positionMs: Math.max(0, Math.round(positionMs)) }),
  playlists: (): Promise<PlaylistsView> =>
    isTauri ? invoke("music_playlists") : Promise.resolve({ connected: true, favorites: FAKE_LISTS.slice(0, 6), all: FAKE_LISTS }),
  setFavorites: (ids: string[]): Promise<void> => isTauri ? invoke("music_set_favorites", { ids }) : Promise.resolve(),
  playPlaylist: (id: string) => call("music_play_playlist", { id }),
  transfer: (deviceId: string) => call("music_transfer", { deviceId }),
  /** Appareils Spotify ; liste vide si l'appel échoue. */
  devices: async (): Promise<Device[]> => {
    if (!isTauri) return FAKE_DEVICES;
    const out = await invoke<ToolOutcome>("music_devices");
    return "Done" in out ? ((out.Done.output as { devices?: Device[] }).devices ?? []) : [];
  },
};
