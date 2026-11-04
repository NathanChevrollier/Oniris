import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Panel } from "./Panel";
import { usePanelStore } from "./store";
import { ipc, type PendingAction } from "../lib/ipc";

const panelClose = vi.fn();
beforeEach(() => {
  panelClose.mockReset();
  panelClose.mockResolvedValue(undefined);
  vi.spyOn(ipc, "panelClose").mockImplementation(panelClose as never);
  usePanelStore.setState({ open: true, pending: [] });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });

it("Échap ferme le panneau", async () => {
  const user = userEvent.setup();
  render(<Panel />);
  await user.keyboard("{Escape}");
  expect(panelClose).toHaveBeenCalledOnce();
});

it("clic sur le voile ferme, clic dans le panneau non", async () => {
  const user = userEvent.setup();
  const { container } = render(<Panel />);
  await user.click(screen.getByText("IRIS arrive bientôt."));
  await user.click(container.querySelector(".panel")!);
  expect(panelClose).not.toHaveBeenCalled();
  await user.click(container.querySelector(".scrim")!);
  expect(panelClose).toHaveBeenCalledOnce();
});

it("rien ne tourne quand le panneau est fermé ; body.closed suit l'état", () => {
  // setImmediate exclu : l'environnement de test (jsdom/vitest) en programme un pour lui-même.
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
  usePanelStore.setState({ open: false });
  render(<Panel />);
  expect(document.body).toHaveClass("closed");
  expect(vi.getTimerCount()).toBe(0);
  act(() => usePanelStore.setState({ open: true }));
  expect(document.body).not.toHaveClass("closed");
  expect(vi.getTimerCount()).toBeGreaterThan(0);
  act(() => usePanelStore.setState({ open: false }));
  expect(vi.getTimerCount()).toBe(0);
});

it("l'horloge avance chaque seconde sans re-rendre l'en-tête", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 10, 3, 16, 40, 52));
  render(<Panel />);
  const greet = screen.getByRole("heading", { level: 1 });
  const before = greet.textContent;
  expect(screen.getByText(":52")).toBeInTheDocument();
  act(() => { vi.advanceTimersByTime(1000); });
  expect(screen.getByText(":53")).toBeInTheDocument();
  expect(screen.getByRole("heading", { level: 1 })).toBe(greet);
  expect(greet.textContent).toBe(before);
  expect(greet).toHaveTextContent("Bonjour");
});

it("la barre de commande prend le focus à chaque ouverture et envoie le texte", () => {
  usePanelStore.setState({ open: false });
  const onCommand = vi.fn();
  render(<Panel onCommand={onCommand} />);
  const input = screen.getByRole("textbox", { name: "Commande" });
  act(() => usePanelStore.setState({ open: true }));
  expect(input).toHaveFocus();
  act(() => { input.blur(); usePanelStore.setState({ open: false }); });
  act(() => usePanelStore.setState({ open: true }));
  expect(input).toHaveFocus();
  fireEvent.change(input, { target: { value: "  Mets ma playlist  " } });
  fireEvent.submit(input.closest("form")!);
  expect(onCommand).toHaveBeenCalledWith("Mets ma playlist");
  expect(input).toHaveValue("");
});

it("frise : une colonne par heure ; grille d'emplacements « Bientôt »", () => {
  const { container } = render(<Panel />);
  const hours = [...container.querySelectorAll(".tl .hours > div")].map((d) => d.textContent);
  expect(hours[0]).toBe("8h");
  expect(hours.at(-1)).toBe("22h");
  expect(container.querySelectorAll(".grid > .card")).toHaveLength(9);
  expect(screen.getAllByText("Bientôt")).toHaveLength(8); // l'agenda a sa carte
});

it("une confirmation en attente s'affiche au-dessus de la grille", () => {
  const p: PendingAction = { action_id: 7, tool: "t", text: { summary: "Supprimer x", details: "d", risk: "Low" }, provenance: "User", expires_at_ms: 0 };
  usePanelStore.setState({ pending: [p] });
  const { container } = render(<Panel />);
  const confirm = screen.getByText("Supprimer x");
  expect(confirm.compareDocumentPosition(container.querySelector(".grid")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});
