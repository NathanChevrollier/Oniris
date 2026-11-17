import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { pcIpc, QuickControls } from "./QuickControls";
import { usePanelStore } from "./store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => usePanelStore.setState({ open: true }));

function setup(bluetooth: boolean | null = false) {
  vi.spyOn(pcIpc, "sound").mockResolvedValue({ volume: { percent: 40, muted: false } });
  vi.spyOn(pcIpc, "state").mockResolvedValue({ volume: { percent: 40, muted: false }, bluetooth });
  const power = vi.spyOn(pcIpc, "power").mockResolvedValue();
  render(<QuickControls />);
  return { power };
}

it("l'extinction demande un second clic, le verrouillage non", async () => {
  const { power } = setup();
  await userEvent.click(screen.getByRole("button", { name: "Alimentation" }));
  await userEvent.click(screen.getByRole("button", { name: "Éteindre" }));
  expect(power).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Confirmer l'extinction" }));
  expect(power).toHaveBeenCalledWith("shutdown");
  await userEvent.click(screen.getByRole("button", { name: "Alimentation" }));
  await userEvent.click(screen.getByRole("button", { name: "Verrouiller" }));
  expect(power).toHaveBeenLastCalledWith("lock");
});

it("volume, muet et Bluetooth relus puis envoyés", async () => {
  setup(false);
  const vol = vi.spyOn(pcIpc, "setVolume").mockResolvedValue();
  const mute = vi.spyOn(pcIpc, "setMute").mockResolvedValue();
  const bt = vi.spyOn(pcIpc, "setBluetooth").mockResolvedValue();
  const slider = await screen.findByRole("slider", { name: "Volume" });
  expect(slider).toHaveProperty("value", "40");
  fireEvent.change(slider, { target: { value: "30" } });
  expect(vol).toHaveBeenCalledWith(30);
  await userEvent.click(screen.getByRole("button", { name: "Couper le son" }));
  expect(mute).toHaveBeenCalledWith(true);
  await userEvent.click(screen.getByRole("button", { name: "Activer le Bluetooth" }));
  expect(bt).toHaveBeenCalledWith(true);
});

it("sans adaptateur, le Bluetooth est désactivé", async () => {
  setup(null);
  expect(await screen.findByRole("button", { name: "Bluetooth indisponible" })).toHaveProperty("disabled", true);
});

it("son indisponible : raison au survol ; molette ±2 % sur le curseur", async () => {
  vi.spyOn(pcIpc, "state").mockResolvedValue({ volume: null, volume_error: "son : élément introuvable", bluetooth: null });
  vi.spyOn(pcIpc, "sound").mockResolvedValue({ volume: null, volume_error: "son : élément introuvable" });
  render(<QuickControls />);
  const slider = await screen.findByRole("slider", { name: "Volume" });
  await vi.waitFor(() => expect(slider.closest("[title]")?.getAttribute("title")).toBe("Son indisponible : son : élément introuvable"));
  cleanup();
  setup();
  vi.spyOn(pcIpc, "sound").mockResolvedValue({ volume: { percent: 40, muted: false }, output: "Casque" });
  const vol = vi.spyOn(pcIpc, "setVolume").mockResolvedValue();
  const s2 = await screen.findByRole("slider", { name: "Volume" });
  await vi.waitFor(() => expect(s2).toHaveProperty("value", "40"));
  fireEvent.wheel(s2, { deltaY: -100 });
  expect(vol).toHaveBeenLastCalledWith(42);
  await vi.waitFor(() => expect(s2).toHaveProperty("value", "42"));
  fireEvent.wheel(s2, { deltaY: 100 });
  await vi.waitFor(() => expect(vol).toHaveBeenLastCalledWith(40));
});

it("mélangeur : une ligne par application, curseur et muet par appli", async () => {
  setup();
  vi.spyOn(pcIpc, "sound").mockResolvedValue({ volume: { percent: 40, muted: false } });
  vi.spyOn(pcIpc, "apps").mockResolvedValue([{ key: "discord", name: "Discord", percent: 80, muted: false, active: true, icon: null }]);
  vi.spyOn(pcIpc, "outputs").mockResolvedValue([{ id: "x", name: "Casque", percent: 100, muted: false, default: true }]);
  const setApp = vi.spyOn(pcIpc, "setApp").mockResolvedValue();
  await userEvent.click(screen.getByRole("button", { name: "Mélangeur" }));
  const s = await screen.findByRole("slider", { name: "Volume de Discord" });
  fireEvent.change(s, { target: { value: "30" } });
  expect(setApp).toHaveBeenCalledWith("discord", 30, null);
  await userEvent.click(screen.getByRole("button", { name: "Couper Discord" }));
  await vi.waitFor(() => expect(setApp).toHaveBeenLastCalledWith("discord", null, true));
  expect(screen.getByRole("slider", { name: "Volume de Casque" })).toBeInTheDocument();
});
