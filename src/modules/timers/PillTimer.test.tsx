import { render, screen, cleanup, act } from "@testing-library/react";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { PillTimer } from "./PillTimer";
import { timersIpc, type Timer } from "./api";

let changed: (e: { payload: { topic: string } }) => void = () => {};
vi.mock("../../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../../lib/ipc")>()),
  listen: (_: string, cb: typeof changed) => { changed = cb; return Promise.resolve(() => {}); },
}));

const timer = (id: number, label: string, ends: number): Timer =>
  ({ id, label, kind: "timer", duration_ms: 1_500_000, ends_ms: ends, focus_cycle: null });
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date(2026, 10, 4, 10, 0, 0, 0)); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });

it("sans minuteur : rien d'affiché et aucune minuterie active", async () => {
  vi.spyOn(timersIpc, "list").mockResolvedValue([]);
  const { container } = render(<PillTimer />);
  await flush();
  expect(container).toBeEmptyDOMElement();
  expect(vi.getTimerCount()).toBe(0);
});

it("avec un minuteur : affiche 24:59 après une seconde", async () => {
  vi.spyOn(timersIpc, "list").mockResolvedValue([timer(1, "a", Date.now() + 1_500_000)]);
  render(<PillTimer />);
  await flush();
  expect(screen.getByText("25:00")).toBeInTheDocument();
  expect(vi.getTimerCount()).toBe(1);
  act(() => { vi.advanceTimersByTime(1000); });
  expect(screen.getByText("24:59")).toBeInTheDocument();
});

it("le premier finit : le suivant s'affiche, puis plus rien et plus aucune minuterie", async () => {
  const list = vi.spyOn(timersIpc, "list").mockResolvedValue([timer(1, "a", Date.now() + 2000), timer(2, "b", Date.now() + 90_000)]);
  render(<PillTimer />);
  await flush();
  expect(screen.getByText("00:02")).toBeInTheDocument();
  act(() => { vi.advanceTimersByTime(2000); });
  expect(screen.getByText("01:28")).toBeInTheDocument();
  list.mockResolvedValue([]);
  act(() => changed({ payload: { topic: "timers" } }));
  await flush();
  expect(list).toHaveBeenCalledTimes(2);
  expect(screen.queryByText(/:/)).toBeNull();
  expect(vi.getTimerCount()).toBe(0);
});

it("un autre sujet ne recharge pas la liste", async () => {
  const list = vi.spyOn(timersIpc, "list").mockResolvedValue([]);
  render(<PillTimer />);
  await flush();
  act(() => changed({ payload: { topic: "notes" } }));
  expect(list).toHaveBeenCalledTimes(1);
});
