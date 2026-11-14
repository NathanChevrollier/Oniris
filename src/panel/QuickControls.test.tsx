import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { pcIpc, QuickControls } from "./QuickControls";
import { usePanelStore } from "./store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => usePanelStore.setState({ open: true }));

function setup(bluetooth: boolean | null = false) {
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
