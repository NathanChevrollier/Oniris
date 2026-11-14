import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Pill } from "./Pill";
import { ipc, type PanelState } from "../lib/ipc";
import type { VoiceEvent } from "../modules/voice/api";
import { timersIpc } from "../modules/timers/api";
import { voiceIpc } from "../modules/voice/api";
import { updateIpc, type UpdatePhase, type UpdateStatus } from "../modules/settings/update";
import { DEFAULT_SETTINGS } from "../lib/settings";

let panelState: (p: PanelState) => void = () => {};
vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  onPanelState: (cb: (p: PanelState) => void) => { panelState = cb; return Promise.resolve(() => {}); },
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    if (name === "oniris://voice") { const prev = voiceCb; voiceCb = (payload) => { prev(payload); cb({ payload }); }; }
    if (name === "oniris://update") updateCb = (payload) => cb({ payload });
    return Promise.resolve(() => {});
  },
}));

let updateCb: (e: UpdateStatus) => void = () => {};
let voiceCb: (e: VoiceEvent) => void = () => {};
const settingsReads = vi.fn();
vi.mock("../lib/settings", async (orig) => {
  const m = await orig<typeof import("../lib/settings")>();
  return { ...m, useSettings: () => { settingsReads(); return m.useSettings(); } };
});

let roCb: ResizeObserverCallback = () => {};
const fire = (w: number) =>
  act(() => roCb([{ borderBoxSize: [{ inlineSize: w, blockSize: 48 }] } as unknown as ResizeObserverEntry], {} as ResizeObserver));
const spies = { pillResize: vi.fn(), panelOpen: vi.fn(), panelToggle: vi.fn() };

beforeEach(() => {
  voiceCb = () => {};
  vi.useFakeTimers();
  vi.spyOn(timersIpc, "list").mockResolvedValue([]); // la pilule charge ses minuteurs (données factices hors Tauri)
  vi.stubGlobal("ResizeObserver", class {
    constructor(cb: ResizeObserverCallback) { roCb = cb; }
    observe() {} unobserve() {} disconnect() {}
  });
  for (const [k, f] of Object.entries(spies)) {
    f.mockReset();
    f.mockResolvedValue(undefined);
    vi.spyOn(ipc, k as keyof typeof spies).mockImplementation(f as never);
  }
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });

it("la pilule demande sa taille au Rust", () => {
  render(<Pill />);
  fire(211.4);
  fire(211.6);
  act(() => { vi.advanceTimersByTime(200); });
  expect(spies.pillResize).toHaveBeenCalledOnce();
  expect(spies.pillResize).toHaveBeenCalledWith(212, false);
  fire(211.8); // même largeur arrondie : pas de nouvel appel
  act(() => { vi.advanceTimersByTime(200); });
  expect(spies.pillResize).toHaveBeenCalledOnce();
});

it("affiche l'heure à la minute avec un seul minuteur aligné", () => {
  vi.setSystemTime(new Date(2026, 10, 3, 8, 42, 30));
  render(<Pill />);
  expect(screen.getByText("08:42")).toBeInTheDocument();
  expect(vi.getTimerCount()).toBe(1);
  act(() => { vi.advanceTimersByTime(29_999); });
  expect(screen.getByText("08:42")).toBeInTheDocument();
  act(() => { vi.advanceTimersByTime(1); });
  expect(screen.getByText("08:43")).toBeInTheDocument();
  expect(vi.getTimerCount()).toBe(1);
});

it("survol prolongé : ouvre le panneau ; quitter avant annule", () => {
  render(<Pill />);
  const btn = screen.getByRole("button");
  fireEvent.mouseEnter(btn);
  act(() => { vi.advanceTimersByTime(300); });
  fireEvent.mouseLeave(btn);
  act(() => { vi.advanceTimersByTime(1000); });
  expect(spies.panelOpen).not.toHaveBeenCalled();
  fireEvent.mouseEnter(btn);
  act(() => { vi.advanceTimersByTime(350); });
  expect(spies.panelOpen).toHaveBeenCalledOnce();
});

it("survol juste après la fermeture du panneau : ignoré 600 ms", () => {
  render(<Pill />);
  const btn = screen.getByRole("button");
  act(() => panelState({ open: false, at_ms: 0 }));
  fireEvent.mouseEnter(btn); // le curseur se retrouve sur la pilule quand le panneau disparaît
  act(() => { vi.advanceTimersByTime(1000); });
  expect(spies.panelOpen).not.toHaveBeenCalled();
  fireEvent.mouseLeave(btn);
  fireEvent.mouseEnter(btn);
  act(() => { vi.advanceTimersByTime(350); });
  expect(spies.panelOpen).toHaveBeenCalledOnce();
});

it("clic : bascule le panneau ; l'orbe est au repos", () => {
  const { container } = render(<Pill />);
  fireEvent.click(screen.getByRole("button"));
  expect(spies.panelToggle).toHaveBeenCalledOnce();
  expect(container.querySelector(".orb")).toHaveAttribute("data-state", "idle");
});

it("emplacements cachés de la musique (plan 3) et des mails (plan 4)", () => {
  const { container } = render(<Pill />);
  expect(container.querySelector(".slot-music")).not.toBeVisible();
  expect(container.querySelector(".slot-mails")).not.toBeVisible();
});

it("le niveau du micro ne redessine que l'orbe, pas la pilule", () => {
  render(<Pill />);
  const orb = () => document.querySelector(".orb") as HTMLElement;
  const before = settingsReads.mock.calls.length;
  for (const level of [0.2, 0.5, 0.9]) act(() => voiceCb({ phase: "listening", level }));
  expect(orb().style.getPropertyValue("--level")).toBe("0.9");
  expect(settingsReads.mock.calls.length).toBe(before);
});

const withLearning = async (on: boolean, wake = true) => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue({ ...DEFAULT_SETTINGS, voice_wake: wake, voice_wake_learning: on });
  render(<Pill />);
  await act(async () => {});
};

it("bouton « Je vais dire Iris » seulement en mode apprentissage", async () => {
  await withLearning(false);
  expect(screen.queryByRole("button", { name: "Je vais dire Iris" })).toBeNull();
  cleanup();
  const expectWake = vi.spyOn(voiceIpc, "expectWake").mockResolvedValue(undefined);
  await withLearning(true);
  const b = screen.getByRole("button", { name: "Je vais dire Iris" });
  fireEvent.click(b);
  expect(expectWake).toHaveBeenCalledOnce();
  expect(b).toHaveAttribute("data-armed", "true");
  act(() => { vi.advanceTimersByTime(5000); });
  expect(b).not.toHaveAttribute("data-armed");
});

it("mode apprentissage gardé mais mot d'activation coupé : aucun bouton", async () => {
  await withLearning(true, false);
  expect(screen.queryByRole("button", { name: "Je vais dire Iris" })).toBeNull();
  act(() => voiceCb({ phase: "listening", by_wake: true }));
  expect(screen.queryByRole("button", { name: "Faux réveil" })).toBeNull();
});

it("« Faux réveil » apparaît 10 s après un réveil par la voix", async () => {
  const falseWake = vi.spyOn(voiceIpc, "falseWake").mockResolvedValue(undefined);
  await withLearning(true);
  expect(screen.queryByRole("button", { name: "Faux réveil" })).toBeNull();
  act(() => voiceCb({ phase: "listening", level: 0.2 })); // push-to-talk : pas de bouton
  expect(screen.queryByRole("button", { name: "Faux réveil" })).toBeNull();
  act(() => voiceCb({ phase: "listening", by_wake: true }));
  act(() => voiceCb({ phase: "transcribing" }));
  fireEvent.click(screen.getByRole("button", { name: "Faux réveil" }));
  expect(falseWake).toHaveBeenCalledOnce();
  expect(screen.queryByRole("button", { name: "Faux réveil" })).toBeNull(); // le clic le retire
});

it("« Faux réveil » disparaît seul au bout de 10 s", async () => {
  await withLearning(true);
  act(() => voiceCb({ phase: "listening", by_wake: true }));
  expect(screen.getByRole("button", { name: "Faux réveil" })).toBeInTheDocument();
  act(() => { vi.advanceTimersByTime(9_900); });
  expect(screen.getByRole("button", { name: "Faux réveil" })).toBeInTheDocument();
  act(() => { vi.advanceTimersByTime(200); });
  expect(screen.queryByRole("button", { name: "Faux réveil" })).toBeNull();
});

const upd = (p: UpdatePhase, more: Partial<UpdateStatus> = {}) =>
  ({ current: "1.2.0", pending: null, checking: false, last_error: null, updated: null, ...p, ...more }) as UpdateStatus;
const ready121 = upd({ state: "ready", version: "1.2.1", notes: "" }, { pending: ["1.2.1", ""] });

it("la pilule suit la mise à jour : téléchargement, prête, attente, installation", () => {
  const restart = vi.spyOn(updateIpc, "restart").mockResolvedValue(undefined);
  render(<Pill />);
  act(() => updateCb(upd({ state: "checking" })));
  expect(document.querySelector(".pill-update")).toBeNull(); // une simple vérification reste silencieuse
  act(() => updateCb(upd({ state: "downloading", version: "1.2.1", percent: null })));
  expect(screen.getByText("Téléchargement de la mise à jour 1.2.1…")).toBeTruthy();
  act(() => updateCb(upd({ state: "downloading", version: "1.2.1", percent: 42 })));
  expect(screen.getByText("Téléchargement de la mise à jour 1.2.1… 42 %")).toBeTruthy();
  act(() => updateCb(ready121));
  expect(screen.getByText("Mise à jour 1.2.1 prête")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Redémarrer" }));
  expect(restart).toHaveBeenCalled();
  act(() => updateCb(upd({ state: "waiting", version: "1.2.1", reason: "game" })));
  expect(screen.getByText("En attente : tu joues")).toBeTruthy();
  act(() => updateCb(upd({ state: "waiting", version: "1.2.1", reason: "voice" })));
  expect(screen.getByText("En attente : tu parles à IRIS")).toBeTruthy();
  act(() => updateCb(upd({ state: "installing", version: "1.2.1" })));
  expect(screen.getByText("Installation… Oniris va redémarrer")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Redémarrer" })).toBeNull();
});

it("l'annonce reste discrète et se ferme", () => {
  render(<Pill />);
  act(() => updateCb(ready121));
  fireEvent.click(screen.getByRole("button", { name: "Plus tard" }));
  expect(screen.queryByText("Mise à jour 1.2.1 prête")).toBeNull();
  act(() => updateCb(upd({ state: "checking" }, { pending: ["1.2.1", ""] })));
  act(() => updateCb(ready121)); // même état après une vérification : toujours masquée
  expect(screen.queryByText("Mise à jour 1.2.1 prête")).toBeNull();
});

it("échec lisible avec « Réessayer » ; une vérification ratée en fond ne s'affiche pas", () => {
  const check = vi.spyOn(updateIpc, "checkNow").mockResolvedValue(upd({ state: "idle" }));
  const restart = vi.spyOn(updateIpc, "restart").mockResolvedValue(undefined);
  render(<Pill />);
  act(() => updateCb(upd({ state: "error", message: "GitHub ne répond pas.", version: null })));
  expect(document.querySelector(".pill-update")).toBeNull();
  act(() => updateCb(upd({ state: "error", message: "Le téléchargement a échoué.", version: "1.2.1" })));
  fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
  expect(check).toHaveBeenCalledOnce();
  act(() => updateCb(upd({ state: "error", message: "L'installateur n'a pas pu être lancé.", version: "1.2.1" }, { pending: ["1.2.1", ""] })));
  expect(screen.getByText("L'installateur n'a pas pu être lancé.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
  expect(restart).toHaveBeenCalledOnce();
});

it("la pilule relit une mise à jour déjà prête au démarrage", async () => {
  vi.spyOn(updateIpc, "status").mockResolvedValue(ready121);
  render(<Pill />);
  await act(async () => {});
  expect(screen.getByText("Mise à jour 1.2.1 prête")).toBeTruthy();
});

it("après une mise à jour : « Oniris est passé en X » avec « Nouveautés », une fois", async () => {
  const whatsNew = vi.spyOn(updateIpc, "whatsNew").mockResolvedValue(undefined);
  vi.spyOn(updateIpc, "status").mockResolvedValue(upd({ state: "idle" }, { current: "1.2.1", updated: { version: "1.2.1", notes: "Nouveau" } }));
  render(<Pill />);
  await act(async () => {});
  expect(screen.getByText("Oniris est passé en 1.2.1")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Nouveautés" }));
  expect(whatsNew).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "Fermer" }));
  expect(screen.queryByText("Oniris est passé en 1.2.1")).toBeNull();
});

it("veille : bande fine, réveil au contact du curseur ou de la voix, puis rendormie", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue({ ...DEFAULT_SETTINGS, pill_sleep: true });
  const { container } = render(<Pill />);
  await act(async () => {});
  const root = container.querySelector(".pill-root")!;
  expect(root).toHaveClass("asleep");
  act(() => { vi.advanceTimersByTime(300); });
  expect(spies.pillResize).toHaveBeenLastCalledWith(420, true);
  fireEvent.mouseEnter(root);
  expect(root).not.toHaveClass("asleep");
  expect(spies.pillResize).toHaveBeenLastCalledWith(420, false);
  fireEvent.mouseLeave(root);
  act(() => { vi.advanceTimersByTime(800); });
  expect(root).toHaveClass("asleep");
  act(() => voiceCb({ phase: "listening" } as VoiceEvent));
  expect(root).not.toHaveClass("asleep");
  act(() => voiceCb({ phase: "idle" } as VoiceEvent));
  act(() => panelState({ open: true, at_ms: 0 }));
  expect(root).not.toHaveClass("asleep");
});
