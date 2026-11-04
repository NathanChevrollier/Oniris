import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { CommandPalette, highlight, parseQuery, searchIpc, shortPath, type Hit, type SearchResult } from "./CommandPalette";
import { Panel } from "./Panel";
import { usePanelStore } from "./store";
import { ipc, invoke } from "../lib/ipc";
import { timersIpc } from "../modules/timers/api";

// Comme dans Panel.test : « dans Tauri », `invoke` ne répond jamais (les cartes restent vides).
vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  isTauri: true,
  invoke: vi.fn(() => new Promise(() => {})),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const ok = { Done: { action_id: 1, output: {} } };
const hit = (kind: Hit["item"]["kind"], key: string, title: string, subtitle = ""): Hit => ({ item: { kind, key, title, subtitle }, score: 1 });
const HITS: Hit[] = [
  hit("file", "C:\\u\\docs\\rapport.pdf", "rapport.pdf", "C:\\u\\docs"),
  hit("app", "C:\\m\\Rapports.lnk", "Rapports", "Application"),
  hit("dir", "C:\\u\\rapports", "rapports", "C:\\u"),
];
/** `search_query` qui répond tout de suite avec `hits` et le bon numéro de requête. */
const answer = (hits: Hit[] = HITS) =>
  vi.spyOn(searchIpc, "query").mockImplementation((_q, _s, req) => Promise.resolve({ req, hits, indexing: false }));

beforeEach(() => usePanelStore.setState({ open: true, pending: [] }));
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.mocked(invoke).mockClear(); });

const bar = () => screen.getByRole("combobox", { name: "Commande" });
const selected = () => screen.getAllByRole("option").find((o) => o.getAttribute("aria-selected") === "true");

it("aucune recherche à l'ouverture : seulement à la frappe", async () => {
  const q = answer();
  render(<CommandPalette />);
  expect(bar()).toHaveFocus();
  expect(q).not.toHaveBeenCalled();
  await userEvent.type(bar(), "r");
  expect(q).toHaveBeenCalledWith("r", null, expect.any(Number));
});

it("navigation au clavier : ↑/↓ bornés, Entrée ouvre, Ctrl+Entrée ouvre le dossier parent", async () => {
  answer();
  const open = vi.spyOn(searchIpc, "open").mockResolvedValue("done");
  render(<CommandPalette />);
  await userEvent.type(bar(), "rapport");
  expect(await screen.findAllByRole("option")).toHaveLength(3);
  expect(selected()).toHaveTextContent("rapport.pdf");
  expect(bar()).toHaveAttribute("aria-activedescendant", selected()!.id);
  await userEvent.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}");
  expect(selected()).toHaveTextContent("rapports");
  expect(selected()).not.toHaveTextContent("Application");
  await userEvent.keyboard("{ArrowUp}");
  expect(selected()).toHaveTextContent("Rapports");
  await userEvent.keyboard("{Enter}");
  expect(open).toHaveBeenCalledWith("app", "C:\\m\\Rapports.lnk");
  await userEvent.type(bar(), "rapport");
  await screen.findAllByRole("option");
  await userEvent.keyboard("{Control>}{Enter}{/Control}");
  expect(open).toHaveBeenLastCalledWith("dir", "C:\\u\\docs");
});

it("les réponses périmées sont ignorées", async () => {
  const pending: ((r: SearchResult) => void)[] = [];
  const reqs: number[] = [];
  vi.spyOn(searchIpc, "query").mockImplementation((_q, _s, req) => { reqs.push(req); return new Promise((res) => pending.push(res)); });
  render(<CommandPalette />);
  await userEvent.type(bar(), "ra", { skipClick: true }); // un clic dans la barre vide chargerait les récents
  expect(reqs).toHaveLength(2);
  expect(reqs[1]).toBeGreaterThan(reqs[0]);
  await act(async () => pending[1]({ req: reqs[1], hits: [HITS[1]], indexing: false }));
  await act(async () => pending[0]({ req: reqs[0], hits: [HITS[0], HITS[2]], indexing: false }));
  expect(screen.getAllByRole("option")).toHaveLength(1);
  expect(selected()).toHaveTextContent("Rapports");
});

it("les préfixes de portée sont transmis", async () => {
  const q = answer();
  render(<CommandPalette />);
  for (const [text, scope] of [["p oni", "project"], ["f oni", "file"], ["a oni", "app"], ["b oni", "favorite"], ["n oni", "note"]]) {
    fireEvent.change(bar(), { target: { value: text } });
    expect(q).toHaveBeenLastCalledWith("oni", scope, expect.any(Number));
  }
  expect(parseQuery("pain")).toEqual({ q: "pain", scope: null });
  expect(parseQuery("/minuteur 10 min")).toEqual({ q: "minuteur 10 min", scope: "command" });
});

it("/minuteur 10 min appelle timers.start(\"10 min\") sans recherche côté Rust", async () => {
  const q = answer();
  const start = vi.spyOn(timersIpc, "start").mockResolvedValue(ok);
  vi.spyOn(searchIpc, "open").mockResolvedValue("command");
  render(<CommandPalette />);
  await userEvent.type(bar(), "/minuteur 10 min", { skipClick: true });
  expect(q).not.toHaveBeenCalled();
  expect(screen.getByRole("option")).toHaveTextContent("/minuteur");
  await userEvent.keyboard("{Enter}");
  expect(start).toHaveBeenCalledWith("10 min");
  expect(bar()).toHaveValue("");
});

it("/minu + Entrée complète la commande qui attend un argument", async () => {
  render(<CommandPalette />);
  await userEvent.type(bar(), "/minu{Enter}");
  expect(bar()).toHaveValue("/minuteur ");
});

it("Ctrl+P dans le panneau donne le focus à la barre avec « p »", async () => {
  const q = answer([]);
  render(<CommandPalette />);
  act(() => bar().blur());
  await userEvent.keyboard("{Control>}p{/Control}");
  expect(bar()).toHaveFocus();
  expect(bar()).toHaveValue("p ");
  expect(q).toHaveBeenLastCalledWith("", "project", expect.any(Number));
});

it("Échap vide le champ, puis ferme le panneau", async () => {
  answer();
  const close = vi.spyOn(ipc, "panelClose").mockResolvedValue();
  render(<Panel />);
  await userEvent.type(bar(), "rapport");
  await screen.findAllByRole("option");
  await userEvent.keyboard("{Escape}");
  expect(bar()).toHaveValue("");
  expect(screen.queryByRole("option")).toBeNull();
  expect(close).not.toHaveBeenCalled();
  await userEvent.keyboard("{Escape}");
  expect(close).toHaveBeenCalledOnce();
});

it("texte sans résultat validé par Entrée : message d'attente d'IRIS", async () => {
  answer([]);
  const onCommand = vi.fn();
  render(<CommandPalette onCommand={onCommand} />);
  await userEvent.type(bar(), "mets ma playlist");
  await act(async () => {});
  await userEvent.keyboard("{Enter}");
  expect(onCommand).toHaveBeenCalledWith("mets ma playlist");
  expect(screen.getByText("IRIS arrive bientôt — essaie /note, /rappel…")).toBeInTheDocument();
});

it("un exécutable en attente de confirmation : barre vidée, sans erreur", async () => {
  answer([hit("file", "C:\\u\\setup.exe", "setup.exe", "C:\\u")]);
  vi.spyOn(searchIpc, "open").mockResolvedValue("pending");
  render(<CommandPalette />);
  await userEvent.type(bar(), "setup");
  await screen.findByRole("option");
  await userEvent.keyboard("{Enter}");
  expect(bar()).toHaveValue("");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("surlignage par sous-séquence et chemin raccourci", () => {
  expect(highlight("rapport.pdf", "rpdf")).toEqual([0, 2, 9, 10]);
  expect(highlight("Oniris", "ONI")).toEqual([0, 1, 2]);
  expect(highlight("todo-oni", "oni")).toEqual([5, 6, 7]); // sous-chaîne préférée à la sous-séquence
  expect(highlight("abc", "xyz")).toEqual([]);
  expect(highlight("Météo", "meteo")).toEqual([0, 1, 2, 3, 4]);
  expect(shortPath("C:\\Users\\natha\\Desktop\\Utilitaires\\Projets\\Oniris\\docs")).toBe("…\\Projets\\Oniris\\docs");
  expect(shortPath("C:\\u\\docs")).toBe("C:\\u\\docs");
});

it("récents : chargés au premier geste dans la barre vide (clic ou ↓), jamais à l'ouverture", async () => {
  const q = answer();
  render(<CommandPalette />);
  expect(q).not.toHaveBeenCalled();
  await userEvent.keyboard("{ArrowDown}");
  expect(q).toHaveBeenCalledWith("", null, expect.any(Number));
  expect(await screen.findAllByRole("option")).toHaveLength(3);
});
