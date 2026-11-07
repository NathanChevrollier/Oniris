import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Panel } from "./Panel";
import { searchIpc } from "./CommandPalette";
import { usePanelStore } from "./store";
import { invoke, ipc, listen, type PendingAction } from "../lib/ipc";

// Le panneau se croit dans Tauri : chaque module passe alors par `invoke` (au lieu des données factices),
// ce qui rend les appels IPC observables. `invoke` ne répond jamais, `listen` abonne à vide.
vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  isTauri: true,
  invoke: vi.fn(() => new Promise(() => {})),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));
const invokeMock = vi.mocked(invoke);

const panelClose = vi.fn();
beforeEach(() => {
  invokeMock.mockClear();
  vi.mocked(listen).mockClear();
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
  await user.click(screen.getByText("IRIS prépare ton briefing…"));
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

it("la barre de commande prend le focus à chaque ouverture et envoie le texte", async () => {
  vi.spyOn(searchIpc, "query").mockImplementation((_q, _s, req) => Promise.resolve({ req, hits: [], indexing: false }));
  usePanelStore.setState({ open: false });
  const onCommand = vi.fn();
  render(<Panel onCommand={onCommand} />);
  const input = screen.getByRole("combobox", { name: "Commande" });
  act(() => usePanelStore.setState({ open: true }));
  expect(input).toHaveFocus();
  act(() => { input.blur(); usePanelStore.setState({ open: false }); });
  act(() => usePanelStore.setState({ open: true }));
  expect(input).toHaveFocus();
  fireEvent.change(input, { target: { value: "  Mets ma playlist  " } });
  await act(async () => {}); // réponse (vide) de la recherche
  fireEvent.keyDown(input, { key: "Enter" });
  expect(onCommand).toHaveBeenCalledWith("Mets ma playlist");
  expect(input).toHaveValue("");
});

it("frise : une colonne par heure ; grille d'emplacements « Bientôt »", () => {
  const { container } = render(<Panel />);
  const hours = [...container.querySelectorAll(".tl .hours > div")].map((d) => d.textContent);
  expect(hours[0]).toBe("8h");
  expect(hours.at(-1)).toBe("22h");
  expect(container.querySelectorAll(".grid > .card")).toHaveLength(9);
  expect(screen.queryAllByText("Bientôt")).toHaveLength(0); // musique, mails, agenda, projets, notes, presse-papiers, minuteur, Ce PC et VPS et favoris ont leur carte
});

it("les 9 emplacements, dans l'ordre de la maquette (l'agenda est titré par le mois)", () => {
  const { container } = render(<Panel />);
  const month = new Date().toLocaleDateString("fr-FR", { month: "long" });
  const titles = [...container.querySelectorAll(".grid > .card > h3")].map((h) => h.firstChild?.textContent);
  expect(titles).toEqual(["En lecture", "Mails", month[0].toUpperCase() + month.slice(1), "Projets", "Ce PC", "VPS et favoris", "Notes", "Presse-papiers", "Minuteur"]);
});

it("panneau fermé : ni minuterie ni appel IPC, seulement des abonnements", () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
  usePanelStore.setState({ open: false });
  const getSettings = vi.spyOn(ipc, "getSettings");
  render(<Panel />);
  // Seule lecture : les réglages (`useSettings`), au montage de la fenêtre, jamais ensuite.
  const settingsReads = getSettings.mock.calls.length;
  act(() => { vi.advanceTimersByTime(5 * 60_000); });
  expect(vi.getTimerCount()).toBe(0);
  expect(invokeMock).not.toHaveBeenCalled();
  expect(getSettings).toHaveBeenCalledTimes(settingsReads);
  expect(vi.mocked(listen).mock.calls.map(([e]) => e)).not.toContain("oniris://system");
});

it("à l'ouverture, chaque sujet se charge après le premier rendu (dans un effet)", () => {
  // Monté ouvert : un appel fait pendant le rendu ne trouverait pas encore le panneau dans le document.
  const seenGrid: boolean[] = [];
  invokeMock.mockImplementation(() => {
    seenGrid.push(document.querySelectorAll(".panel .grid > .card").length === 9);
    return new Promise(() => {});
  });
  render(<Panel />);
  const cmds = new Set(invokeMock.mock.calls.map(([c]) => c));
  for (const c of ["agenda_between", "agenda_month_days", "reminders_list", "notes_list", "clipboard_list", "timers_list", "weather_get"]) {
    expect(cmds).toContain(c);
  }
  expect(seenGrid.length).toBe(invokeMock.mock.calls.length);
  expect(seenGrid.every(Boolean)).toBe(true); // le DOM du panneau était déjà affiché à chaque appel
});

it("une confirmation en attente s'affiche au-dessus de la grille", () => {
  const p: PendingAction = { action_id: 7, tool: "t", text: { summary: "Supprimer x", details: "d", risk: "Low" }, provenance: "User", expires_at_ms: 0 };
  usePanelStore.setState({ pending: [p] });
  const { container } = render(<Panel />);
  const confirm = screen.getByText("Supprimer x");
  expect(confirm.compareDocumentPosition(container.querySelector(".grid")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});
