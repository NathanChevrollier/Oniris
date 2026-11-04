import { render, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { WeatherBadge } from "./WeatherBadge";
import { weatherIpc, fakeWeather } from "./api";
import { ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS } from "../../lib/settings";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => usePanelStore.setState({ open: true }));

const withCity = (weather_city: string) => vi.spyOn(ipc, "getSettings").mockResolvedValue({ ...DEFAULT_SETTINGS, weather_city });

it("ville vide : « Choisir une ville »", async () => {
  withCity("");
  render(<WeatherBadge />);
  expect(screen.getByRole("button", { name: "Choisir une ville" })).toBeInTheDocument();
});

it("Entrée enregistre la ville par settings_save ; Échap annule sans fermer le panneau", async () => {
  withCity("");
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  const user = userEvent.setup();
  const onWindowKey = vi.fn();
  window.addEventListener("keydown", onWindowKey);
  render(<WeatherBadge />);
  await user.click(screen.getByRole("button", { name: "Choisir une ville" }));
  await user.keyboard("{Escape}");
  expect(onWindowKey).not.toHaveBeenCalled();
  expect(screen.queryByLabelText("Ville de la météo")).toBeNull();
  expect(save).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Choisir une ville" }));
  await user.type(screen.getByLabelText("Ville de la météo"), "  Lyon {Enter}");
  expect(save).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, weather_city: "Lyon" });
  window.removeEventListener("keydown", onWindowKey);
});

it("pluie à venir : « pluie vers 18 h », température arrondie et ville", async () => {
  withCity("Paris");
  vi.spyOn(weatherIpc, "get").mockResolvedValue({ ...fakeWeather(), temp_c: 11.6 });
  const { container } = render(<WeatherBadge />);
  expect(await screen.findByText("12° · pluie vers 18 h")).toBeInTheDocument();
  expect(screen.getByText("Paris")).toBeInTheDocument();
  expect(container.querySelector(".w")).not.toHaveClass("stale");
});

it("donnée de 4 h : grisée (stale) avec l'heure de la mesure", async () => {
  withCity("Paris");
  const at = new Date(Date.now() - 4 * 3600_000);
  vi.spyOn(weatherIpc, "get").mockResolvedValue({ ...fakeWeather(), fetched_ms: at.getTime(), rain_from_hour: null });
  const { container } = render(<WeatherBadge />);
  await waitFor(() => expect(container.querySelector(".w")).toHaveClass("stale"));
  const hm = `${String(at.getHours()).padStart(2, "0")}h${String(at.getMinutes()).padStart(2, "0")}`;
  expect(screen.getByText(`à ${hm}`)).toBeInTheDocument();
  expect(screen.getByText("12° · Pluie faible")).toBeInTheDocument();
});

it("hors Tauri : données factices de la maquette", async () => {
  expect((await weatherIpc.get())?.place).toBe("Paris");
});
