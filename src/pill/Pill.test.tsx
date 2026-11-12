import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Pill } from "./Pill";
import { ipc, type PanelState } from "../lib/ipc";
import type { VoiceEvent } from "../modules/voice/api";
import { timersIpc } from "../modules/timers/api";
import { voiceIpc } from "../modules/voice/api";
import { updateIpc } from "../modules/settings/update";
import { DEFAULT_SETTINGS } from "../lib/settings";

let panelState: (p: PanelState) => void = () => {};
vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  onPanelState: (cb: (p: PanelState) => void) => { panelState = cb; return Promise.resolve(() => {}); },
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    if (name === "oniris://voice") voiceCb = (payload) => cb({ payload });
    if (name === "oniris://update") updateCb = (payload) => cb({ payload });
    return Promise.resolve(() => {});
  },
}));

let updateCb: (e: { version: string; notes: string }) => void = () => {};
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
  expect(spies.pillResize).toHaveBeenCalledWith(212);
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

it("la pilule annonce une mise à jour prête", () => {
  const restart = vi.spyOn(updateIpc, "restart").mockResolvedValue(undefined);
  render(<Pill />);
  act(() => updateCb({ version: "1.2.1", notes: "" }));
  expect(screen.getByText("Mise à jour 1.2.1 prête")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Redémarrer" }));
  expect(restart).toHaveBeenCalled();
});

it("l'annonce reste discrète et se ferme", () => {
  render(<Pill />);
  act(() => updateCb({ version: "1.2.1", notes: "" }));
  fireEvent.click(screen.getByRole("button", { name: "Plus tard" }));
  expect(screen.queryByText("Mise à jour 1.2.1 prête")).toBeNull();
});
