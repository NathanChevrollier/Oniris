import { render, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { MonthGrid } from "./MonthGrid";
import { DayList } from "./DayList";
import { AgendaCard } from "./AgendaCard";
import { agendaIpc } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("MonthGrid : le 1er novembre 2026 (dimanche) est en 7e colonne, jour courant et points marqués", () => {
  const { container } = render(<MonthGrid year={2026} month={11} today={4} selected={null} withEvents={[4, 12]} onSelect={() => {}} />);
  const cells = [...container.querySelectorAll(".cal > span, .cal > button")];
  expect(cells.findIndex((c) => c.textContent === "1")).toBe(6); // 6 cases vides avant le dimanche
  expect(container.querySelectorAll(".cal > .d")).toHaveLength(7);
  expect(screen.getByRole("button", { name: "4" })).toHaveClass("today", "has");
  expect(screen.getByRole("button", { name: "12" })).toHaveClass("has");
  expect(screen.getByRole("button", { name: "5" })).not.toHaveClass("today", "has");
  expect(container.querySelectorAll(".cal button")).toHaveLength(30);
});

it("MonthGrid : un clic sélectionne le jour", async () => {
  const onSelect = vi.fn();
  render(<MonthGrid year={2026} month={11} today={null} selected={3} withEvents={[]} onSelect={onSelect} />);
  await userEvent.click(screen.getByRole("button", { name: "9" }));
  expect(onSelect).toHaveBeenCalledWith(9);
});

it("DayList vide : « Rien de prévu »", () => {
  render(<DayList events={[]} onDelete={() => {}} />);
  expect(screen.getByText("Rien de prévu")).toBeInTheDocument();
});

it("AgendaCard : une date introuvable affiche l'erreur en français", async () => {
  usePanelStore.setState({ open: true });
  vi.spyOn(agendaIpc, "monthDays").mockResolvedValue([]);
  vi.spyOn(agendaIpc, "between").mockResolvedValue([]);
  vi.spyOn(agendaIpc, "create").mockResolvedValue({ Failed: { action_id: 1, error: "arguments invalides : date introuvable" } });
  render(<AgendaCard />);
  await userEvent.type(screen.getByRole("textbox", { name: "Ajouter un événement" }), "dentiste{Enter}");
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Date introuvable — essaie « demain 14h dentiste »"));
});

beforeEach(() => usePanelStore.setState({ open: false }));
