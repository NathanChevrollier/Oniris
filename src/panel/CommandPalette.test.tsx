import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { CommandPalette, highlight, parseQuery, searchIpc, shortPath, type Hit, type SearchResult } from "./CommandPalette";
import { Panel } from "./Panel";
import { usePanelStore } from "./store";
import { ipc, invoke } from "../lib/ipc";
import { timersIpc } from "../modules/timers/api";
import { irisIpc } from "../modules/iris/api";
import { useIris } from "../modules/iris/store";

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

beforeEach(() => { usePanelStore.setState({ open: true, pending: [] }); useIris.setState({ turn: 0, floor: 0, state: "idle", user: "", segments: [], error: null, toolsUsed: [], pending: [], brain: null }); });
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

it("projet : Entrée ouvre le dossier, Ctrl+Entrée reprend avec Claude", async () => {
  answer([hit("project", "C:\\p\\Oniris", "Oniris", "Task 8")]);
  const open = vi.spyOn(searchIpc, "open").mockResolvedValue("done");
  const resume = vi.spyOn(searchIpc, "resume").mockResolvedValue("Lance Claude manuellement : claude --continue");
  render(<CommandPalette />);
  await userEvent.type(bar(), "oni");
  await screen.findAllByRole("option");
  expect(screen.getByText(/reprendre avec Claude/)).toBeInTheDocument();
  await userEvent.keyboard("{Control>}{Enter}{/Control}");
  expect(resume).toHaveBeenCalledWith("C:\\p\\Oniris", "normal");
  expect(open).not.toHaveBeenCalled();
  await act(async () => {});
  expect(screen.getByRole("status")).toHaveTextContent("Lance Claude manuellement : claude --continue");
  await userEvent.keyboard("{Enter}");
  expect(open).toHaveBeenCalledWith("project", "C:\\p\\Oniris");
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

it("texte libre sans résultat : Entrée interroge IRIS, Ctrl+Maj+Entrée force Claude", async () => {
  answer([]);
  const ask = vi.spyOn(irisIpc, "ask").mockResolvedValue(1);
  render(<CommandPalette />);
  await userEvent.type(bar(), "mets ma playlist");
  await act(async () => {});
  await userEvent.keyboard("{Enter}");
  expect(ask).toHaveBeenCalledWith("mets ma playlist", false, false);
  expect(bar()).toHaveValue("");
  expect(useIris.getState()).toMatchObject({ user: "mets ma playlist", state: "thinking" });
  await userEvent.type(bar(), "résume", { skipClick: true });
  await act(async () => {});
  await userEvent.keyboard("{Control>}{Shift>}{Enter}{/Shift}{/Control}");
  expect(ask).toHaveBeenLastCalledWith("résume", true, false);
});

it("Entrée sur un résultat sélectionné l'ouvre sans solliciter IRIS", async () => {
  answer();
  const ask = vi.spyOn(irisIpc, "ask").mockResolvedValue(1);
  const open = vi.spyOn(searchIpc, "open").mockResolvedValue("done");
  render(<CommandPalette />);
  await userEvent.type(bar(), "rapport");
  await screen.findAllByRole("option");
  await userEvent.keyboard("{Enter}");
  expect(open).toHaveBeenCalledWith("file", "C:\\u\\docs\\rapport.pdf");
  expect(ask).not.toHaveBeenCalled();
});

it("Maj+Entrée seul n'interroge pas IRIS : il ouvre le résultat ; Ctrl+Entrée sur une commande l'exécute", async () => {
  answer();
  const ask = vi.spyOn(irisIpc, "ask").mockResolvedValue(1);
  const open = vi.spyOn(searchIpc, "open").mockResolvedValue("done");
  render(<CommandPalette />);
  await userEvent.type(bar(), "rapport");
  await screen.findAllByRole("option");
  await userEvent.keyboard("{Shift>}{Enter}{/Shift}");
  expect(open).toHaveBeenCalledWith("file", "C:\\u\\docs\\rapport.pdf");
  expect(ask).not.toHaveBeenCalled();
  const start = vi.spyOn(timersIpc, "focus").mockResolvedValue(ok as never);
  await userEvent.type(bar(), "/focus");
  await userEvent.keyboard("{Control>}{Enter}{/Control}");
  expect(start).toHaveBeenCalled();
  expect(ask).not.toHaveBeenCalled();
});

it("le bouton Claude force Claude", async () => {
  answer([]);
  const ask = vi.spyOn(irisIpc, "ask").mockResolvedValue(1);
  render(<CommandPalette />);
  await userEvent.type(bar(), "résume mes mails");
  await userEvent.click(screen.getByRole("button", { name: "Claude" }));
  expect(ask).toHaveBeenCalledWith("résume mes mails", true, false);
});

it("Échap pendant un tour l'annule sans fermer le panneau", async () => {
  answer([]);
  const close = vi.spyOn(ipc, "panelClose").mockResolvedValue(undefined);
  vi.spyOn(irisIpc, "ask").mockResolvedValue(7);
  const cancel = vi.spyOn(irisIpc, "cancel").mockResolvedValue(undefined);
  render(<Panel />);
  await userEvent.type(bar(), "bonjour");
  await act(async () => {});
  await userEvent.keyboard("{Enter}");
  await act(async () => {});
  await userEvent.keyboard("{Escape}");
  await act(async () => {});
  expect(cancel).toHaveBeenCalledWith(7);
  expect(close).not.toHaveBeenCalled();
  expect(useIris.getState()).toMatchObject({ user: "", state: "idle" });
});

it("un exécutable en attente de confirmation : barre vidée, sans erreur", async () => {
  answer([hit("file", "C:\\u\\setup.exe", "setup.exe", "C:\\u")]);
  vi.spyOn(searchIpc, "open").mockResolvedValue("pending");
  render(<CommandPalette />);
  await userEvent.type(bar(), "setup");
  await screen.findByRole("option");
  await userEvent.keyboard("{Enter}");
  expect(bar()).toHaveValue("");
  expect(screen.getByRole("alert")).toBeEmptyDOMElement();
});

it("surlignage fidèle : sous-chaîne, sinon mot par mot, sinon rien ; chemin raccourci", () => {
  expect(highlight("Réunion client jeudi", "oni")).toEqual([]); // sous-séquence éparpillée : aucune marque
  expect(highlight("rapport.pdf", "rpdf")).toEqual([]);
  expect(highlight("Oniris", "ONI")).toEqual([0, 1, 2]);
  expect(highlight("todo-oni", "oni")).toEqual([5, 6, 7]);
  expect(highlight("plan-01-fondations.md", "plan fond")).toEqual([0, 1, 2, 3, 8, 9, 10, 11]);
  expect(highlight("fondations plan", "plan fond")).toEqual([0, 1, 2, 3, 11, 12, 13, 14]);
  expect(highlight("plan-01", "plan !old")).toEqual([0, 1, 2, 3]); // mots négatifs ignorés
  expect(highlight("plan-01", "plan zzz")).toEqual([]); // un mot absent : rien
  expect(highlight("Météo", "meteo")).toEqual([0, 1, 2, 3, 4]);
  // Accents décomposés (NFD) : indices sur la chaîne d'origine, marques combinantes incluses.
  expect(highlight("Météo", "météo")).toEqual([0, 1, 2, 3, 4, 5, 6]);
  expect(highlight("xét", "e")).toEqual([1, 2]);
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

it("une correspondance dans le sous-titre seulement y est surlignée", async () => {
  answer([hit("note", "2", "Réunion client jeudi", "Préparer la démo Oniris")]);
  render(<CommandPalette />);
  await userEvent.type(bar(), "oni", { skipClick: true });
  const opt = await screen.findByRole("option");
  expect(opt.querySelector(".pal-title mark")).toBeNull();
  expect([...opt.querySelectorAll(".pal-sub mark")].map((m) => m.textContent).join("")).toBe("Oni");
});

it("une Entrée en attente est oubliée si l'utilisateur retape avant la réponse", async () => {
  const pending: ((r: SearchResult) => void)[] = [];
  const reqs: number[] = [];
  vi.spyOn(searchIpc, "query").mockImplementation((_q, _s, req) => { reqs.push(req); return new Promise((res) => pending.push(res)); });
  const open = vi.spyOn(searchIpc, "open").mockResolvedValue("done");
  render(<CommandPalette />);
  await userEvent.type(bar(), "r{Enter}a", { skipClick: true });
  await act(async () => pending[1]({ req: reqs[1], hits: HITS, indexing: false }));
  expect(open).not.toHaveBeenCalled();
  expect(screen.getAllByRole("option")).toHaveLength(3);
});

it("la fenêtre de résultats n'est montée que s'il y a quelque chose à montrer", async () => {
  answer([hit("note", "2", "Réunion", "x")]);
  const { container } = render(<CommandPalette />);
  expect(container.querySelector(".pal-pop")).toBeNull();
  await userEvent.type(bar(), "reu", { skipClick: true });
  await screen.findByRole("option");
  expect(container.querySelector(".pal-pop")).not.toBeNull();
  await userEvent.keyboard("{Escape}");
  expect(container.querySelector(".pal-pop")).toBeNull();
});

it("zones d'annonce (status, alert) toujours montées : seul leur contenu change", async () => {
  answer([]);
  render(<CommandPalette />);
  const status = screen.getByRole("status");
  const alert = screen.getByRole("alert");
  expect(status).toBeEmptyDOMElement();
  expect(alert).toBeEmptyDOMElement();
  await userEvent.type(bar(), "/zzz", { skipClick: true });
  await act(async () => {});
  await userEvent.keyboard("{Enter}");
  expect(screen.getByRole("status")).toBe(status);
  expect(status).toHaveTextContent("Commande inconnue");
  expect(screen.getByRole("alert")).toBe(alert);
});
