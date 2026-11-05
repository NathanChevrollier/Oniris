import { render, screen, cleanup, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { MonthGrid } from "./MonthGrid";
import { DayList } from "./DayList";
import { AgendaCard } from "./AgendaCard";
import { agendaIpc } from "./api";
import { usePanelStore } from "../../panel/store";
import { Timeline } from "../../panel/Timeline";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
beforeEach(() => usePanelStore.setState({ open: false }));

it("MonthGrid : le 1er novembre 2026 (dimanche) est en 7e colonne, jour courant et points marqués", () => {
  const { container } = render(<MonthGrid year={2026} month={11} today={4} selected={null} withEvents={[4, 12]} onSelect={() => {}} />);
  const cells = [...container.querySelectorAll(".cal > span, .cal > button")];
  expect(cells.findIndex((c) => c.textContent === "1")).toBe(6); // 6 cases vides avant le dimanche
  expect(container.querySelectorAll(".cal > .d")).toHaveLength(7);
  expect(screen.getByRole("button", { name: /^4 novembre 2026/ })).toHaveClass("today", "has");
  expect(screen.getByRole("button", { name: /^12 novembre 2026/ })).toHaveClass("has");
  expect(screen.getByRole("button", { name: /^5 novembre 2026/ })).not.toHaveClass("today", "has");
  expect(container.querySelectorAll(".cal button")).toHaveLength(30);
});

it("MonthGrid : accessibilité (libellés de jour, aujourd'hui, en-têtes)", () => {
  const { container } = render(<MonthGrid year={2026} month={11} today={4} selected={null} withEvents={[4]} onSelect={() => {}} />);
  expect(screen.getByRole("button", { name: "4 novembre 2026, avec des événements" })).toHaveAttribute("aria-current", "date");
  expect(screen.getByRole("button", { name: "5 novembre 2026" })).not.toHaveAttribute("aria-current");
  expect(container.querySelector('abbr[title="lundi"]')).toHaveTextContent("L");
  expect(container.querySelector('abbr[title="dimanche"]')).toHaveTextContent("D");
});

it("MonthGrid : un clic sélectionne le jour", async () => {
  const onSelect = vi.fn();
  render(<MonthGrid year={2026} month={11} today={null} selected={3} withEvents={[]} onSelect={onSelect} />);
  await userEvent.click(screen.getByRole("button", { name: /^9 novembre 2026/ }));
  expect(onSelect).toHaveBeenCalledWith(9);
});

it("DayList vide : « Rien de prévu, journée libre. »", () => {
  render(<DayList events={[]} onDelete={() => {}} />);
  expect(screen.getByText("Rien de prévu, journée libre.")).toBeInTheDocument();
});

function mockRead() {
  vi.spyOn(agendaIpc, "monthDays").mockResolvedValue([]);
  return vi.spyOn(agendaIpc, "between").mockResolvedValue([]);
}

it("AgendaCard : une date introuvable affiche l'erreur en français", async () => {
  usePanelStore.setState({ open: true });
  mockRead();
  vi.spyOn(agendaIpc, "create").mockResolvedValue({ Failed: { action_id: 1, error: "arguments invalides : date introuvable" } });
  render(<AgendaCard />);
  await userEvent.type(screen.getByRole("textbox", { name: "Ajouter un événement" }), "dentiste{Enter}");
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Date introuvable — essaie « demain 14h dentiste »"));
});

it("AgendaCard : une commande en erreur affiche un message inline", async () => {
  usePanelStore.setState({ open: true });
  mockRead();
  vi.spyOn(agendaIpc, "create").mockRejectedValue(new Error("boom"));
  render(<AgendaCard />);
  await userEvent.type(screen.getByRole("textbox", { name: "Ajouter un événement" }), "demain 10h x{Enter}");
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Agenda indisponible"));
});

it("AgendaCard : le jour affiché suit minuit tant qu'aucun jour n'est choisi", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.setSystemTime(new Date(2026, 10, 4, 23, 59, 30));
  usePanelStore.setState({ open: true });
  const between = mockRead();
  render(<AgendaCard />);
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByRole("button", { name: /^4 novembre/ })).toHaveAttribute("aria-current", "date");
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(screen.getByRole("button", { name: /^5 novembre/ })).toHaveAttribute("aria-current", "date");
  expect(between.mock.calls.length).toBeGreaterThanOrEqual(2);
});

it("événement Google : sans bouton de suppression dans la liste, bloc coloré dans la frise", async () => {
  const base = { end_ms: null, all_day: false, notes: null };
  const local = { ...base, id: 3, key: "l:3", source: "local" as const, readonly: false, title: "Dentiste", start_ms: 1, color: null };
  const google = { ...base, id: 0, key: "g:1:c:e", source: "google" as const, readonly: true, title: "Réunion", start_ms: 2, color: "#0a84ff" };
  const onDelete = vi.fn();
  render(<DayList events={[local, google]} onDelete={onDelete} />);
  expect(screen.getByRole("button", { name: "Supprimer Dentiste" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Supprimer Réunion" })).toBeNull();
  cleanup();
  const at = (h: number) => new Date(2026, 10, 5, h);
  const { container } = render(<Timeline startHour={8} endHour={20} items={[{ start: at(10), end: at(11), label: "Réunion", kind: "event", color: "#0a84ff", readonly: true }]} />);
  const ev = container.querySelector(".ev") as HTMLElement;
  expect(ev).toHaveTextContent("Réunion");
  expect(ev.style.getPropertyValue("--c")).toBe("#0a84ff");
  expect(ev).toHaveAttribute("data-readonly", "true");
  expect(ev.querySelector("button")).toBeNull();
});
