import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Ambient, MusicCard, positionAt } from "./MusicCard";
import { musicIpc, type NowPlaying, type PlaylistsView } from "./api";
import { usePanelStore } from "../../panel/store";
import type { ToolOutcome } from "../../lib/ipc";

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
  expect(set).toHaveBeenLastCalledWith(["p2"]);
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
