import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { TimerCard } from "./TimerCard";
import { alarmsIpc, alarmWhen, isAlarmText, timersIpc, type Alarm, type Timer } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => {
  usePanelStore.setState({ open: true });
  vi.spyOn(alarmsIpc, "state").mockResolvedValue({ alarms: [], ringing: [] });
});

const ok = { Done: { action_id: 1, output: {} } };
const timer = (id: number, label: string, min: number): Timer =>
  ({ id, label, kind: "timer", duration_ms: min * 60_000, ends_ms: Date.now() + min * 60_000, focus_cycle: null });

it("le bouton « 25 min » appelle timers_start(\"25 minutes\"), « Focus » lance un focus", async () => {
  vi.spyOn(timersIpc, "list").mockResolvedValue([]);
  const start = vi.spyOn(timersIpc, "start").mockResolvedValue(ok);
  const focus = vi.spyOn(timersIpc, "focus").mockResolvedValue(ok);
  render(<TimerCard />);
  await userEvent.click(screen.getByRole("button", { name: "25 min" }));
  expect(start).toHaveBeenCalledWith("25 minutes");
  await userEvent.click(screen.getByRole("button", { name: "5 min" }));
  expect(start).toHaveBeenLastCalledWith("5 minutes");
  await userEvent.click(screen.getByRole("button", { name: "Focus" }));
  expect(focus).toHaveBeenCalledOnce();
});

it("anneau du premier minuteur avec mm:ss et libellé, autres minuteurs annulables", async () => {
  vi.spyOn(timersIpc, "list").mockResolvedValue([timer(1, "Thé", 4), timer(2, "Pâtes", 10)]);
  const cancel = vi.spyOn(timersIpc, "cancel").mockResolvedValue(ok);
  const { container } = render(<TimerCard />);
  expect(await screen.findByText("Thé")).toBeInTheDocument();
  expect(screen.getByText("04:00")).toBeInTheDocument();
  expect(container.querySelector("svg circle.arc")).not.toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Annuler Pâtes" }));
  expect(cancel).toHaveBeenCalledWith(2);
  await userEvent.click(screen.getByRole("button", { name: "Arrêter Thé" }));
  expect(cancel).toHaveBeenLastCalledWith(1);
});

it("le champ libre lance un minuteur et affiche l'échec", async () => {
  vi.spyOn(timersIpc, "list").mockResolvedValue([]);
  const start = vi.spyOn(timersIpc, "start").mockResolvedValueOnce(ok)
    .mockResolvedValueOnce({ Failed: { action_id: 2, error: "durée introuvable" } });
  render(<TimerCard />);
  const input = screen.getByRole("textbox", { name: "Nouveau minuteur" });
  await userEvent.type(input, "1h30 pâtes{Enter}");
  expect(start).toHaveBeenCalledWith("1h30 pâtes");
  await userEvent.type(input, "bientôt{Enter}");
  expect(await screen.findByRole("alert")).toHaveTextContent("Durée introuvable — essaie « 10 min pâtes »");
});

const alarm = (id: number, label: string, days = 0): Alarm =>
  ({ id, label, hour: 7, minute: 5, days, next_ms: new Date(2026, 10, 5, 7, 5).getTime(), repeat: days ? "en semaine" : "une fois" });

it("le champ reconnaît une alarme, la liste l'affiche et la supprime", async () => {
  vi.spyOn(timersIpc, "list").mockResolvedValue([]);
  vi.spyOn(alarmsIpc, "state").mockResolvedValue({ alarms: [alarm(4, "Réveil", 0x1f)], ringing: [] });
  const set = vi.spyOn(alarmsIpc, "set").mockResolvedValue(ok);
  const start = vi.spyOn(timersIpc, "start").mockResolvedValue(ok);
  const cancel = vi.spyOn(alarmsIpc, "cancel").mockResolvedValue(ok);
  render(<TimerCard />);
  expect(await screen.findByText("07:05 · en semaine")).toBeInTheDocument();
  await userEvent.type(screen.getByRole("textbox", { name: "Nouveau minuteur" }), "réveille-moi à 7 h{Enter}");
  expect(set).toHaveBeenCalledWith("réveille-moi à 7 h");
  expect(start).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Supprimer l'alarme Réveil" }));
  expect(cancel).toHaveBeenCalledWith(4);
});

it("alarme qui sonne : Arrêter et +5 min", async () => {
  vi.spyOn(timersIpc, "list").mockResolvedValue([]);
  vi.spyOn(alarmsIpc, "state").mockResolvedValue({ alarms: [], ringing: [{ id: 1, label: "Réveil", due_ms: 0 }] });
  const stop = vi.spyOn(alarmsIpc, "stop").mockResolvedValue();
  const snooze = vi.spyOn(alarmsIpc, "snooze").mockResolvedValue();
  render(<TimerCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Arrêter" }));
  expect(stop).toHaveBeenCalledOnce();
  await userEvent.click(screen.getByRole("button", { name: "Répéter dans 5 minutes" }));
  expect(snooze).toHaveBeenCalledOnce();
});

it("alarme ou minuteur selon le texte, et libellé de l'échéance", () => {
  for (const t of ["alarme 7h", "Réveille-moi à 7 h", "à 14h30 sport", "demain 8h"]) expect(isAlarmText(t), t).toBe(true);
  for (const t of ["10 min pâtes", "1h30 pâtes", "25 minutes"]) expect(isAlarmText(t), t).toBe(false);
  const now = new Date(2026, 10, 4, 22, 0);
  expect(alarmWhen(alarm(1, "x"), now)).toBe("07:05 · demain");
  expect(alarmWhen(alarm(1, "x", 0x1f), now)).toBe("07:05 · en semaine");
});
