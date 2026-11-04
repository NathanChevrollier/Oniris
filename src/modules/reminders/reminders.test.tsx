import { render, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { ReminderList } from "./ReminderList";
import { remindersIpc, type Reminder } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => usePanelStore.setState({ open: true }));

const soon = Date.now() + 30 * 60_000;
const later = Date.now() + 3 * 24 * 3_600_000;
const list: Reminder[] = [
  { id: 1, text: "Appeler la banque", due_ms: soon, done: false },
  { id: 2, text: "Renouveler le domaine", due_ms: later, done: false },
];

it("cocher un rappel appelle reminders_complete(id, true)", async () => {
  vi.spyOn(remindersIpc, "list").mockResolvedValue(list);
  const complete = vi.spyOn(remindersIpc, "complete").mockResolvedValue({ Done: { action_id: 1, output: {} } });
  render(<ReminderList />);
  await userEvent.click(await screen.findByRole("button", { name: "Terminer Appeler la banque" }));
  expect(complete).toHaveBeenCalledWith(1, true);
});

it("une échéance dans moins de 2 h a la classe soon, pas les autres", async () => {
  vi.spyOn(remindersIpc, "list").mockResolvedValue(list);
  const { container } = render(<ReminderList />);
  await screen.findByText("Appeler la banque");
  const due = [...container.querySelectorAll(".rem small")];
  expect(due[0]).toHaveClass("soon");
  expect(due[1]).not.toHaveClass("soon");
});

it("le champ « Nouveau rappel… » crée un rappel et affiche l'erreur en cas d'échec", async () => {
  vi.spyOn(remindersIpc, "list").mockResolvedValue([]);
  const create = vi.spyOn(remindersIpc, "create").mockResolvedValueOnce({ Done: { action_id: 1, output: {} } })
    .mockRejectedValueOnce(new Error("boom"));
  render(<ReminderList />);
  const input = screen.getByRole("textbox", { name: "Nouveau rappel" });
  await userEvent.type(input, "demain pain{Enter}");
  expect(create).toHaveBeenCalledWith("demain pain");
  await waitFor(() => expect(input).toHaveValue(""));
  await userEvent.type(input, "x{Enter}");
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Rappels indisponibles"));
});

it("un rappel fait du jour est barré avec sa coche verte ; le décocher rappelle complete(id, false)", async () => {
  vi.spyOn(remindersIpc, "list").mockResolvedValue([...list, { id: 3, text: "Sauvegarde VPS", due_ms: soon, done: true }]);
  const complete = vi.spyOn(remindersIpc, "complete").mockResolvedValue({ Done: { action_id: 1, output: {} } });
  const { container } = render(<ReminderList />);
  const btn = await screen.findByRole("button", { name: "Rouvrir Sauvegarde VPS" });
  expect(btn).toHaveAttribute("aria-pressed", "true");
  const row = btn.closest(".rem")!;
  expect(row).toHaveClass("done");
  expect(row.querySelector("small")).toHaveTextContent("fait");
  expect(row.querySelector("small")).not.toHaveClass("soon");
  expect(container.querySelectorAll(".rem.done")).toHaveLength(1);
  await userEvent.click(btn);
  expect(complete).toHaveBeenCalledWith(3, false);
});
