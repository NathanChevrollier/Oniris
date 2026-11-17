import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { NotesCard } from "./NotesCard";
import { notesIpc, type Note } from "./api";
import { usePanelStore } from "../../panel/store";
import { toolIpc } from "../../lib/ipc";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
beforeEach(() => usePanelStore.setState({ open: true }));

const done = { Done: { action_id: 1, output: {} } };
const notes: Note[] = [
  { id: 1, body: "Réunion client", pinned: true, created_ms: 1, updated_ms: Date.now() },
  { id: 2, body: "Penser au pain", pinned: false, created_ms: 1, updated_ms: Date.now() - 3 * 3_600_000 },
];

it("Entrée enregistre la note, Maj+Entrée va à la ligne sans enregistrer", async () => {
  vi.spyOn(notesIpc, "list").mockResolvedValue([]);
  const create = vi.spyOn(notesIpc, "create").mockResolvedValue(done);
  render(<NotesCard />);
  const box = screen.getByRole("textbox", { name: "Nouvelle note" });
  await userEvent.type(box, "ligne un{Shift>}{Enter}{/Shift}ligne deux");
  expect(create).not.toHaveBeenCalled();
  expect(box).toHaveValue("ligne un\nligne deux");
  await userEvent.type(box, "{Enter}");
  expect(create).toHaveBeenCalledWith("ligne un\nligne deux");
  await vi.waitFor(() => expect(box).toHaveValue(""));
});

it("la recherche est temporisée de 150 ms", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const list = vi.spyOn(notesIpc, "list").mockResolvedValue([]);
  render(<NotesCard />);
  await act(async () => {});
  list.mockClear();
  fireEvent.change(screen.getByRole("searchbox", { name: "Rechercher une note" }), { target: { value: "reu" } });
  await act(async () => { vi.advanceTimersByTime(149); });
  expect(list).not.toHaveBeenCalled();
  await act(async () => { vi.advanceTimersByTime(1); });
  expect(list).toHaveBeenCalledWith("reu");
});

it("épingler, mettre en rappel et supprimer appellent l'IPC ; une note épinglée porte un repère", async () => {
  vi.spyOn(notesIpc, "list").mockResolvedValue(notes);
  const pin = vi.spyOn(notesIpc, "pin").mockResolvedValue(done);
  const toReminder = vi.spyOn(notesIpc, "toReminder").mockResolvedValue(done);
  const remove = vi.spyOn(notesIpc, "remove").mockResolvedValue(done);
  render(<NotesCard />);
  await screen.findByText("Penser au pain");
  expect(screen.getAllByLabelText("Épinglée")).toHaveLength(1);
  await userEvent.click(screen.getByRole("button", { name: "Épingler Penser au pain" }));
  expect(pin).toHaveBeenCalledWith(2, true);
  await userEvent.click(screen.getByRole("button", { name: "Désépingler Réunion client" }));
  expect(pin).toHaveBeenCalledWith(1, false);
  await userEvent.click(screen.getByRole("button", { name: "En rappel : Penser au pain" }));
  expect(toReminder).toHaveBeenCalledWith(2);
  await userEvent.click(screen.getByRole("button", { name: "Supprimer Penser au pain" }));
  expect(remove).toHaveBeenCalledWith(2);
});

it("affiche l'erreur quand l'enregistrement échoue", async () => {
  vi.spyOn(notesIpc, "list").mockResolvedValue([]);
  vi.spyOn(notesIpc, "create").mockRejectedValue(new Error("boom"));
  render(<NotesCard />);
  await userEvent.type(screen.getByRole("textbox", { name: "Nouvelle note" }), "x{Enter}");
  expect(await screen.findByRole("alert")).toHaveTextContent("Notes indisponibles");
});

it("les libellés des boutons coupent le texte de la note à 80 caractères", async () => {
  const long = "x".repeat(300);
  vi.spyOn(notesIpc, "list").mockResolvedValue([{ id: 3, body: long, pinned: false, created_ms: 1, updated_ms: Date.now() }]);
  render(<NotesCard />);
  expect(await screen.findByRole("button", { name: `Supprimer ${"x".repeat(80)}` })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: `Épingler ${"x".repeat(80)}` })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: `En rappel : ${"x".repeat(80)}` })).toBeInTheDocument();
});

it("titre et aperçu : la 1re ligne est le titre, la suite l'aperçu", async () => {
  vi.spyOn(notesIpc, "list").mockResolvedValue([{ id: 4, body: "Courses\nlait, pain", pinned: false, created_ms: 1, updated_ms: Date.now() }]);
  const { container } = render(<NotesCard />);
  await screen.findByText("Courses");
  expect(container.querySelector(".ntitle")).toHaveTextContent("Courses");
  expect(container.querySelector(".nprev")).toHaveTextContent("lait, pain");
});

it("édition en place : Entrée enregistre, Maj+Entrée va à la ligne, Échap annule", async () => {
  vi.spyOn(notesIpc, "list").mockResolvedValue(notes);
  const update = vi.spyOn(notesIpc, "update").mockResolvedValue(done);
  render(<NotesCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Modifier Penser au pain" }));
  const box = screen.getByRole("textbox", { name: "Modifier Penser au pain" });
  expect(box).toHaveFocus();
  await userEvent.type(box, "{Shift>}{Enter}{/Shift}complet{Enter}");
  expect(update).toHaveBeenCalledOnce();
  expect(update).toHaveBeenCalledWith(2, "Penser au pain\ncomplet");
  await vi.waitFor(() => expect(screen.getByRole("button", { name: "Modifier Penser au pain" })).toHaveFocus());

  await userEvent.click(screen.getByRole("button", { name: "Modifier Réunion client" }));
  await userEvent.type(screen.getByRole("textbox", { name: "Modifier Réunion client" }), " x{Escape}");
  expect(update).toHaveBeenCalledOnce();
  expect(screen.queryByRole("textbox", { name: "Modifier Réunion client" })).toBeNull();
});

it("supprimer propose « Annuler », qui annule l'action dans le journal", async () => {
  vi.spyOn(notesIpc, "list").mockResolvedValue(notes);
  vi.spyOn(notesIpc, "remove").mockResolvedValue({ Done: { action_id: 42, output: {} } });
  const undo = vi.spyOn(toolIpc, "undo").mockResolvedValue();
  render(<NotesCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Supprimer Penser au pain" }));
  expect(await screen.findByText("Note supprimée.")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Annuler" }));
  expect(undo).toHaveBeenCalledWith(42);
  expect(screen.queryByText("Note supprimée.")).toBeNull();
});

it("le champ grandit avec le texte, bordures comprises, sans barre de défilement", async () => {
  vi.spyOn(notesIpc, "list").mockResolvedValue([]);
  render(<NotesCard />);
  const box = screen.getByRole("textbox", { name: "Nouvelle note" }) as HTMLTextAreaElement;
  // jsdom ne mesure rien : 1 px de bordure en haut et en bas, contenu (padding compris) de 50 px.
  const client = () => (box.style.height && box.style.height !== "auto" ? parseFloat(box.style.height) - 2 : 34);
  Object.defineProperties(box, { scrollHeight: { value: 50, configurable: true }, clientHeight: { get: client, configurable: true }, offsetHeight: { get: () => client() + 2, configurable: true } });
  await userEvent.type(box, "a");
  expect(box.style.height).toBe("52px");
  expect(box.style.overflowY).toBe("hidden");
});
