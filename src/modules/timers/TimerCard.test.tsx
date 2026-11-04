import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { TimerCard } from "./TimerCard";
import { timersIpc, type Timer } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => usePanelStore.setState({ open: true }));

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
