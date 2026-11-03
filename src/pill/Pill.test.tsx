import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Pill } from "./Pill";
import { ipc } from "../lib/ipc";

let roCb: ResizeObserverCallback = () => {};
const fire = (w: number) =>
  act(() => roCb([{ borderBoxSize: [{ inlineSize: w, blockSize: 48 }] } as unknown as ResizeObserverEntry], {} as ResizeObserver));
const spies = { pillResize: vi.fn(), panelOpen: vi.fn(), panelToggle: vi.fn() };

beforeEach(() => {
  vi.useFakeTimers();
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

it("clic : bascule le panneau ; l'orbe est au repos", () => {
  const { container } = render(<Pill />);
  fireEvent.click(screen.getByRole("button"));
  expect(spies.panelToggle).toHaveBeenCalledOnce();
  expect(container.querySelector(".orb")).toHaveAttribute("data-state", "idle");
});
