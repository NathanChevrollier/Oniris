import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Ambient, MusicCard, positionAt } from "./MusicCard";
import { musicIpc, type NowPlaying, type PlaylistsView } from "./api";
import { usePanelStore } from "../../panel/store";
import type { ToolOutcome } from "../../lib/ipc";

// Compte les rendus de la carte (Card est redessinée à chaque rendu de MusicCard).
const cardRenders = vi.hoisted(() => ({ n: 0 }));
vi.mock("../../ui/Card", async (orig) => {
  const real = await orig<typeof import("../../ui/Card")>();
  return { Card: (p: Parameters<typeof real.Card>[0]) => { cardRenders.n++; return real.Card(p); } };
});

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
beforeEach(() => usePanelStore.setState({ open: true }));

const done: ToolOutcome = { Done: { action_id: 1, output: {} } };
const track = (extra: Partial<NowPlaying> = {}): NowPlaying => ({
  title: "Nuit Blanche", artist: "Polo & Pan", album: "Focus", cover: "data:image/png;base64,AAAA", colors: ["#ff7448", "#ff3d7f", "#7b4dff"],
  playing: true, position_ms: 60_000, duration_ms: 200_000, updated_ms: Date.now(), source: "spotify", ...extra,
});
const pl = (id: string) => ({ id, name: `Liste ${id}`, image: null, owner_is_me: true });
const connected = (n = 2): PlaylistsView => ({ connected: true, favorites: Array.from({ length: n }, (_, i) => pl(`p${i + 1}`)), all: [pl("p1"), pl("p2"), pl("p3")] });

function setup(now: NowPlaying | null, view: PlaylistsView = connected(), onAccounts = vi.fn(), onColors = vi.fn()) {
  vi.spyOn(musicIpc, "now").mockResolvedValue(now);
  vi.spyOn(musicIpc, "playlists").mockResolvedValue(view);
  vi.spyOn(musicIpc, "devices").mockResolvedValue([]);
  render(<MusicCard onAccounts={onAccounts} onColors={onColors} />);
  return { onAccounts, onColors };
}

it("les boutons appellent toggle, next et previous", async () => {
  const toggle = vi.spyOn(musicIpc, "toggle").mockResolvedValue(done);
  const next = vi.spyOn(musicIpc, "next").mockResolvedValue(done);
  const prev = vi.spyOn(musicIpc, "previous").mockResolvedValue(done);
  setup(track());
  await userEvent.click(await screen.findByRole("button", { name: "Pause" }));
  await userEvent.click(screen.getByRole("button", { name: "Suivant" }));
  await userEvent.click(screen.getByRole("button", { name: "Précédent" }));
  expect([toggle, next, prev].map((f) => f.mock.calls.length)).toEqual([1, 1, 1]);
});

it("pochette décorative et titre lisible par les lecteurs d'écran", async () => {
  setup(track({ playing: false }));
  expect(await screen.findByRole("button", { name: "Lecture" })).toBeTruthy();
  expect(document.querySelector("img")!.getAttribute("alt")).toBe("");
  expect(document.querySelector(".sr-only")!.textContent).toContain("Nuit Blanche");
});

it("glisser sur la barre appelle seek à la bonne position ; flèches de 5 s", async () => {
  const seek = vi.spyOn(musicIpc, "seek").mockResolvedValue(done);
  setup(track({ playing: false }));
  const bar = await screen.findByRole("slider", { name: "Position" });
  expect(bar.getAttribute("aria-valuemax")).toBe("200000");
  expect(bar.getAttribute("aria-valuenow")).toBe("60000");
  bar.getBoundingClientRect = () => ({ left: 100, width: 200, top: 0, right: 300, bottom: 4, height: 4, x: 100, y: 0, toJSON: () => ({}) });
  fireEvent.pointerDown(bar, { clientX: 100 });
  fireEvent.pointerMove(bar, { clientX: 200 });
  fireEvent.pointerUp(bar, { clientX: 250 });
  expect(seek).toHaveBeenLastCalledWith(150_000);
  await act(async () => {});
  fireEvent.keyDown(bar, { key: "ArrowRight" });
  await act(async () => {});
  fireEvent.keyDown(bar, { key: "ArrowLeft" });
  expect(seek.mock.calls.map((c) => c[0]).slice(-2)).toEqual([155_000, 150_000]);
});

it("la position avance localement et se borne à la durée", () => {
  const np = track({ position_ms: 199_000, updated_ms: 1000 });
  expect(positionAt(np, 1500)).toBe(199_500);
  expect(positionAt(np, 9000)).toBe(200_000);
  expect(positionAt({ ...np, playing: false }, 9000)).toBe(199_000);
});

it("un seul requestAnimationFrame tant que ça joue, aucun en pause ni panneau fermé", async () => {
  const raf = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(1);
  setup(track());
  await screen.findByRole("slider");
  expect(raf).toHaveBeenCalledTimes(1);
  cleanup();
  raf.mockClear();
  setup(track({ playing: false }));
  await screen.findByRole("slider");
  expect(raf).not.toHaveBeenCalled();
  cleanup();
  usePanelStore.setState({ open: false });
  setup(track());
  await act(async () => {});
  expect(raf).not.toHaveBeenCalled();
});

it("un clic sur une vignette lance la playlist", async () => {
  const play = vi.spyOn(musicIpc, "playPlaylist").mockResolvedValue(done);
  setup(track());
  await userEvent.click(await screen.findByRole("button", { name: "Liste p2" }));
  expect(play).toHaveBeenCalledWith("p2");
});

it("le menu des trois points épingle et retire", async () => {
  const set = vi.spyOn(musicIpc, "setFavorites").mockResolvedValue();
  setup(track());
  await userEvent.click(await screen.findByRole("button", { name: "Choisir les playlists" }));
  await userEvent.click(screen.getByRole("checkbox", { name: "Liste p3" }));
  expect(set).toHaveBeenLastCalledWith(["p1", "p2", "p3"]);
  await userEvent.click(screen.getByRole("checkbox", { name: "Liste p1" }));
  expect(set).toHaveBeenLastCalledWith(["p2", "p3"]); // l'épinglage précédent est conservé (mise à jour optimiste)
});

it("l'onde est figée en pause", async () => {
  setup(track({ playing: false }));
  expect((await screen.findByTestId("wave")).className).toContain("paused");
  cleanup();
  setup(track());
  expect((await screen.findByTestId("wave")).className).not.toContain("paused");
});

it("les couleurs sont posées en --ambient-1/2/3 ; sans couleurs, ambiance par défaut", () => {
  const { container, rerender } = render(<Ambient colors={["#111111", "#222222", "#333333"]} />);
  const tint = screen.getByTestId("ambient-tint");
  expect([1, 2, 3].map((i) => tint.style.getPropertyValue(`--ambient-${i}`))).toEqual(["#111111", "#222222", "#333333"]);
  rerender(<Ambient colors={[]} />);
  expect(screen.getByTestId("ambient-tint").className).toContain("out"); // fondu de sortie vers l'ambiance par défaut
  expect(container.querySelector(".ambient")).toBeTruthy();
});

it("onColors reçoit les couleurs de la pochette", async () => {
  const { onColors } = setup(track());
  await screen.findByRole("slider");
  expect(onColors).toHaveBeenLastCalledWith(["#ff7448", "#ff3d7f", "#7b4dff"]);
});

it("rien en lecture : message, pas de commandes, playlists utilisables", async () => {
  setup(null);
  expect(await screen.findByText("Rien en lecture")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Suivant" })).toBeNull();
  expect(await screen.findByRole("button", { name: "Liste p1" })).toBeTruthy();
});

it("Spotify non connecté : pilotage conservé, invitation vers Comptes", async () => {
  const { onAccounts } = setup(track({ source: "other" }), { connected: false, favorites: [], all: [] });
  expect(await screen.findByRole("button", { name: "Pause" })).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Connecte Spotify pour les playlists" }));
  expect(onAccounts).toHaveBeenCalledTimes(1);
});

it("erreur d'une commande : message convivial", async () => {
  vi.spyOn(musicIpc, "next").mockRejectedValue(new Error("boom"));
  setup(track());
  await userEvent.click(await screen.findByRole("button", { name: "Suivant" }));
  expect((await screen.findByRole("alert")).textContent).toBe("Musique indisponible, réessaie.");
});

it("playlists : chargement puis liste vide", async () => {
  vi.spyOn(musicIpc, "now").mockResolvedValue(track());
  vi.spyOn(musicIpc, "playlists").mockReturnValue(new Promise(() => {}));
  vi.spyOn(musicIpc, "devices").mockResolvedValue([]);
  render(<MusicCard />);
  expect(await screen.findByText("Chargement des playlists…")).toBeTruthy();
  cleanup();
  setup(track(), { connected: true, favorites: [], all: [] });
  expect(await screen.findByText("Playlists en cours de chargement…")).toBeTruthy();
});

it("la progression ne redessine que la barre, pas la carte", async () => {
  vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "Date"] });
  vi.spyOn(musicIpc, "now").mockResolvedValue(track());
  vi.spyOn(musicIpc, "playlists").mockResolvedValue(connected());
  vi.spyOn(musicIpc, "devices").mockResolvedValue([]);
  render(<MusicCard />);
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  const bar = screen.getByRole("slider");
  const before = cardRenders.n;
  const v0 = Number(bar.getAttribute("aria-valuenow"));
  await act(async () => { vi.advanceTimersByTime(3000); });
  expect(Number(bar.getAttribute("aria-valuenow"))).toBeGreaterThan(v0 + 2000);
  expect(cardRenders.n).toBe(before);
});

it("la boucle s'arrête en fin de piste et ne démarre pas sans durée", async () => {
  vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "Date"] });
  const raf = vi.spyOn(window, "requestAnimationFrame");
  vi.spyOn(musicIpc, "now").mockResolvedValue(track({ position_ms: 199_000, updated_ms: Date.now() }));
  vi.spyOn(musicIpc, "playlists").mockResolvedValue(connected());
  vi.spyOn(musicIpc, "devices").mockResolvedValue([]);
  render(<MusicCard />);
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  await act(async () => { vi.advanceTimersByTime(3000); });
  const n = raf.mock.calls.length;
  await act(async () => { vi.advanceTimersByTime(3000); });
  expect(raf.mock.calls.length).toBe(n);
  cleanup();
  raf.mockClear();
  setup(track({ duration_ms: 0 }));
  await screen.findByRole("slider");
  expect(raf).not.toHaveBeenCalled();
});

it("durée inconnue : barre désactivée, aucun seek ; Home et End ; seul le bouton principal glisse", async () => {
  const seek = vi.spyOn(musicIpc, "seek").mockResolvedValue(done);
  setup(track({ duration_ms: 0, playing: false }));
  const bar = await screen.findByRole("slider");
  expect(bar.getAttribute("aria-disabled")).toBe("true");
  fireEvent.keyDown(bar, { key: "ArrowRight" });
  fireEvent.keyDown(bar, { key: "End" });
  expect(seek).not.toHaveBeenCalled();
  cleanup();
  setup(track({ playing: false }));
  const b2 = await screen.findByRole("slider");
  fireEvent.keyDown(b2, { key: "End" });
  expect(seek).toHaveBeenLastCalledWith(200_000);
  await act(async () => {});
  fireEvent.keyDown(b2, { key: "Home" });
  expect(seek).toHaveBeenLastCalledWith(0);
  b2.getBoundingClientRect = () => ({ left: 0, width: 100, top: 0, right: 100, bottom: 4, height: 4, x: 0, y: 0, toJSON: () => ({}) });
  seek.mockClear();
  fireEvent.pointerDown(b2, { clientX: 10, button: 2 });
  fireEvent.pointerUp(b2, { clientX: 50, button: 2 });
  expect(seek).not.toHaveBeenCalled();
});

it("un seek en échec ne déplace pas la position affichée", async () => {
  vi.spyOn(musicIpc, "seek").mockResolvedValue({ Failed: { action_id: 1, error: "x" } });
  setup(track({ playing: false }));
  const bar = await screen.findByRole("slider");
  fireEvent.keyDown(bar, { key: "End" });
  await screen.findByRole("alert");
  expect(bar.getAttribute("aria-valuenow")).toBe("60000");
});

it("couleurs invalides : rejetées (ambiance par défaut), aucune injection CSS", async () => {
  const evil = ["#ff0000", "red; background:url(http://x)", "#00ff00"];
  const { onColors } = setup(track({ colors: evil }));
  await screen.findByRole("slider");
  expect(onColors).toHaveBeenLastCalledWith([]);
  expect(document.querySelector<HTMLElement>(".mu-cover")!.getAttribute("style")).not.toContain("url");
  cleanup();
  render(<Ambient colors={evil} />);
  expect(screen.queryByTestId("ambient-tint")).toBeNull();
});

it("appareils : jamais sans Spotify connecté ; playlists et appareils ne se rechargent pas à chaque événement", async () => {
  const devices = vi.spyOn(musicIpc, "devices").mockResolvedValue([]);
  const lists = vi.spyOn(musicIpc, "playlists").mockResolvedValue({ connected: false, favorites: [], all: [] });
  vi.spyOn(musicIpc, "now").mockResolvedValue(track());
  render(<MusicCard />);
  await screen.findByRole("slider");
  expect(devices).not.toHaveBeenCalled();
  cleanup();
  devices.mockClear();
  lists.mockClear();
  const now = vi.spyOn(musicIpc, "now").mockResolvedValue(track());
  lists.mockResolvedValue(connected());
  render(<MusicCard />);
  await screen.findByRole("button", { name: "Liste p1" });
  const l0 = lists.mock.calls.length;
  const d0 = devices.mock.calls.length;
  expect(d0).toBeGreaterThan(0);
  now.mockResolvedValue(track({ playing: false })); // même piste : simple pause
  act(() => usePanelStore.setState({ open: false }));
  act(() => usePanelStore.setState({ open: true }));
  await act(async () => {});
  expect(devices.mock.calls.length).toBe(d0 + 1); // ouverture seulement
  expect(lists.mock.calls.length).toBeLessThanOrEqual(l0 + 1);
});

it("le 9e épinglage est refusé, les erreurs d'enregistrement s'affichent", async () => {
  const eight = Array.from({ length: 8 }, (_, i) => pl(`p${i + 1}`));
  const set = vi.spyOn(musicIpc, "setFavorites").mockResolvedValue();
  setup(track(), { connected: true, favorites: eight, all: [...eight, pl("p9")] });
  await userEvent.click(await screen.findByRole("button", { name: "Choisir les playlists" }));
  await userEvent.click(screen.getByRole("checkbox", { name: "Liste p9" }));
  expect(set).not.toHaveBeenCalled();
  expect((await screen.findByRole("alert")).textContent).toContain("8 playlists au maximum");
  set.mockRejectedValue("non");
  await userEvent.click(screen.getByRole("checkbox", { name: "Liste p1" }));
  expect((await screen.findByRole("alert")).textContent).toContain("Impossible");
});

it("le menu se ferme avec Échap et au clic à l'extérieur", async () => {
  setup(track());
  const more = await screen.findByRole("button", { name: "Choisir les playlists" });
  await userEvent.click(more);
  expect(screen.getByRole("list", { name: "Playlists épinglées" })).toBeTruthy();
  await userEvent.keyboard("{Escape}");
  expect(screen.queryByRole("list", { name: "Playlists épinglées" })).toBeNull();
  await userEvent.click(more);
  await userEvent.click(document.body);
  expect(screen.queryByRole("list", { name: "Playlists épinglées" })).toBeNull();
});

it("chaque barre de l'onde a une hauteur statique propre (forme conservée sans animation)", async () => {
  setup(track());
  const bars = [...(await screen.findByTestId("wave")).querySelectorAll<HTMLElement>("i")];
  expect(new Set(bars.map((b) => b.style.getPropertyValue("--h"))).size).toBeGreaterThan(3);
});

it("Spotify fermé : la vignette affiche le lancement en cours, les autres attendent", async () => {
  let finish = () => {};
  vi.spyOn(musicIpc, "playPlaylist").mockReturnValue(new Promise((r) => { finish = () => r(done); }));
  setup(null);
  const tile = await screen.findByRole("button", { name: "Liste p1" });
  await userEvent.click(tile);
  expect(tile.getAttribute("aria-busy")).toBe("true");
  expect(screen.getByText("Ouverture de Spotify…")).toBeTruthy();
  expect((screen.getByRole("button", { name: "Liste p2" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => finish());
  expect(tile.getAttribute("aria-busy")).toBe("false");
});
