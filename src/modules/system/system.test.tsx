import { render, screen, cleanup, act } from "@testing-library/react";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Gauge } from "./Gauge";
import { PcCard } from "./PcCard";
import { systemIpc, FAKE_STATUS, type SystemStatus } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => usePanelStore.setState({ open: false }));

it("Gauge à 44 % : dashoffset proportionnel à la circonférence", () => {
  const { container } = render(<Gauge label="RAM" value={44} color="#ffd60a" />);
  const c = container.querySelector("circle.fill") as SVGCircleElement;
  const L = 2 * Math.PI * 26;
  expect(parseFloat(c.style.strokeDashoffset)).toBeCloseTo(L * 0.56, 3);
  expect(screen.getByRole("meter", { name: "RAM" })).toHaveAttribute("aria-valuenow", "44");
  expect(container.querySelector("b")).toHaveTextContent("44%");
});

it("Gauge borne à 0..100", () => {
  const { container } = render(<Gauge label="CPU" value={150} color="#fff" />);
  expect(parseFloat((container.querySelector("circle.fill") as SVGCircleElement).style.strokeDashoffset)).toBeCloseTo(0, 5);
});

it("PcCard masque GPU et VRAM quand ils valent null", () => {
  usePanelStore.setState({ open: true });
  let push: (s: SystemStatus) => void = () => {};
  vi.spyOn(systemIpc, "subscribe").mockImplementation((cb) => { push = cb; return () => {}; });
  const snap = vi.spyOn(systemIpc, "snapshot");
  render(<PcCard />);
  expect(screen.getByRole("meter", { name: "GPU" })).toBeInTheDocument();
  expect(snap).not.toHaveBeenCalled(); // pas d'appel supplémentaire à l'ouverture
  act(() => push({ ...FAKE_STATUS, gpu_pct: null, vram_used_mb: null, vram_total_mb: null }));
  expect(screen.queryByRole("meter", { name: "GPU" })).toBeNull();
  expect(screen.queryByRole("meter", { name: "VRAM" })).toBeNull();
  expect(screen.getByRole("meter", { name: "CPU" })).toBeInTheDocument();
  expect(screen.getByText("● 41 Mo")).toBeInTheDocument();
  expect(screen.getAllByText("non installée")).toHaveLength(2);
});

it("PcCard : aucun abonnement panneau fermé, désabonnement à la fermeture", () => {
  const off = vi.fn();
  const sub = vi.spyOn(systemIpc, "subscribe").mockReturnValue(off);
  render(<PcCard />);
  expect(sub).not.toHaveBeenCalled();
  act(() => usePanelStore.setState({ open: true }));
  expect(sub).toHaveBeenCalledTimes(1);
  act(() => usePanelStore.setState({ open: false }));
  expect(off).toHaveBeenCalledTimes(1);
});

it("PcCard : « – » avant le premier tick, et de nouveau après fermeture puis réouverture", () => {
  usePanelStore.setState({ open: true });
  let push: (s: SystemStatus) => void = () => {};
  vi.spyOn(systemIpc, "subscribe").mockImplementation((cb) => { push = cb; return () => {}; });
  render(<PcCard />);
  expect(screen.getByRole("meter", { name: "CPU" }).querySelector("b")).toHaveTextContent("–");
  act(() => push(FAKE_STATUS));
  expect(screen.getByRole("meter", { name: "CPU" }).querySelector("b")).toHaveTextContent("8%");
  act(() => usePanelStore.setState({ open: false }));
  act(() => usePanelStore.setState({ open: true }));
  expect(screen.getByRole("meter", { name: "CPU" }).querySelector("b")).toHaveTextContent("–");
});
