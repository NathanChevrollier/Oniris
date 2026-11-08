import { render, screen, cleanup, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { IrisOrb, IrisReply, IrisStatusLine, IDLE_MS } from "./IrisReply";
import { irisIpc, type IrisEvent } from "./api";
import { apply, ask, useIris, type IrisStore } from "./store";
import { irisBriefingIpc } from "./api";
import { untrustedLabel } from "./Briefing";
import { usePanelStore } from "../../panel/store";

const clean: Partial<IrisStore> = { turn: 0, floor: 0, state: "idle", brain: null, user: "", segments: [], pending: [], error: null, toolsUsed: [], status: null };
beforeEach(() => {
  useIris.setState(clean);
  usePanelStore.setState({ open: true });
  vi.spyOn(irisBriefingIpc, "get").mockResolvedValue(null);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });

type Ev<E = IrisEvent> = E extends unknown ? Omit<E, "turn"> & { turn?: number } : never;
const ev = (e: Ev) => act(() => apply({ turn: 1, ...e } as IrisEvent));
const start = () => { vi.spyOn(irisIpc, "ask").mockResolvedValue(1); act(() => ask("Salut", false)); };

it("les Reply successifs s'affichent, le dernier remplace le précédent", async () => {
  render(<IrisReply />);
  start();
  expect(screen.getByText("Salut")).toBeInTheDocument();
  ev({ type: "state", state: "thinking", brain: "local" });
  ev({ type: "reply", segments: [{ t: "Text", text: "Bon" }], done: false });
  expect(screen.getByText("Bon")).toBeInTheDocument();
  ev({ type: "reply", segments: [{ t: "Text", text: "Bonjour Nathan" }], done: true });
  expect(screen.getByText("Bonjour Nathan")).toBeInTheDocument();
  expect(screen.queryByText("Bon")).toBeNull();
  expect(screen.getByText("local")).toBeInTheDocument();
});

it("un segment Untrusted est du texte, jamais du HTML", () => {
  const { container } = render(<IrisReply />);
  start();
  ev({ type: "reply", segments: [{ t: "Untrusted", text: "<img src=x onerror=alert(1)>", source: "mail:1/a" }], done: true });
  expect(container.querySelector("img")).toBeNull();
  const q = screen.getByText("<img src=x onerror=alert(1)>");
  expect(q).toHaveClass("iris-untrusted");
  expect(q).toHaveAttribute("title", "Contenu de mail, non fiable");
});

it("infobulle selon la source", () => {
  expect(untrustedLabel("agenda externe")).toBe("Événement externe, non fiable");
  expect(untrustedLabel("clipboard")).toBe("Presse-papiers, non fiable");
  expect(untrustedLabel("web:x")).toBe("Contenu externe, non fiable");
});

it("deux demandes avant la réponse de la première : ses événements tardifs sont écartés", async () => {
  let r1!: (n: number) => void;
  vi.spyOn(irisIpc, "ask").mockReturnValueOnce(new Promise((r) => { r1 = r; })).mockResolvedValueOnce(2);
  render(<IrisReply />);
  act(() => ask("A", false));
  act(() => ask("B", false));
  ev({ turn: 1, type: "reply", segments: [{ t: "Text", text: "tardif" }], done: false });
  await act(async () => { r1(1); });
  expect(screen.queryByText("tardif")).toBeNull();
  ev({ turn: 2, type: "reply", segments: [{ t: "Text", text: "neuf" }], done: false });
  expect(screen.getByText("neuf")).toBeInTheDocument();
});

it("statut : le vrai cerveau, y compris le raccourci", () => {
  render(<IrisStatusLine />);
  start();
  ev({ type: "state", state: "thinking", brain: "fast" });
  expect(screen.getByRole("status")).toHaveTextContent("Réfléchit · éclair");
});

it("claude_unavailable : les deux boutons, « local » relance en local seulement", async () => {
  const send = vi.spyOn(irisIpc, "ask").mockResolvedValue(1);
  render(<IrisReply />);
  act(() => ask("Résume", true));
  ev({ type: "state", state: "idle", brain: "claude" });
  ev({ type: "error", kind: "claude_unavailable", message: "Claude est indisponible.", offer_local: true });
  expect(screen.getByRole("alert")).toHaveTextContent("Claude est indisponible.");
  expect(screen.getByRole("button", { name: "Réessayer plus tard" })).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Version simple en local" }));
  expect(send).toHaveBeenLastCalledWith("Résume", false, true);
  expect(useIris.getState().error).toBeNull();
});

it("les événements d'un tour abandonné sont ignorés", () => {
  vi.spyOn(irisIpc, "ask").mockResolvedValue(2);
  render(<IrisReply />);
  start(); // tour 1
  ev({ type: "reply", segments: [{ t: "Text", text: "ancien" }], done: false });
  act(() => ask("Autre", false)); // abandonne le tour 1
  ev({ turn: 1, type: "reply", segments: [{ t: "Text", text: "tardif" }], done: false });
  expect(screen.queryByText("tardif")).toBeNull();
  ev({ turn: 2, type: "reply", segments: [{ t: "Text", text: "neuf" }], done: false });
  expect(screen.getByText("neuf")).toBeInTheDocument();
});

it("outils en puces ; le briefing revient sur « × » et après 2 minutes", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  render(<IrisReply />);
  start();
  ev({ type: "tool_used", tool: "timer_start" });
  ev({ type: "state", state: "idle", brain: "local" });
  expect(screen.getByText("timer_start")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Fermer la réponse" }));
  expect(useIris.getState().user).toBe("");
  vi.mocked(irisIpc.ask).mockResolvedValue(2);
  act(() => ask("Salut", false));
  ev({ turn: 2, type: "state", state: "idle", brain: "local" });
  act(() => { vi.advanceTimersByTime(IDLE_MS + 1); });
  expect(useIris.getState().user).toBe("");
});

it("la sphère passe en thinking pendant un tour et le statut le dit", () => {
  useIris.setState({ status: { local: "ready", model: "m", claude: { kind: "missing" } } });
  const { container } = render(<><IrisOrb size={30} /><IrisStatusLine /></>);
  const orb = container.querySelector(".orb")!;
  expect(orb).toHaveAttribute("data-state", "idle");
  expect(screen.getByRole("status")).toHaveTextContent("Prêt · local");
  start();
  ev({ type: "state", state: "thinking", brain: "claude" });
  expect(orb).toHaveAttribute("data-state", "thinking");
  expect(screen.getByRole("status")).toHaveTextContent("Réfléchit · Claude");
  ev({ type: "state", state: "idle", brain: "claude" });
  expect(orb).toHaveAttribute("data-state", "idle");
});

it("local indisponible", () => {
  useIris.setState({ status: { local: "down", model: "m", claude: { kind: "missing" } } });
  render(<IrisStatusLine />);
  expect(screen.getByRole("status")).toHaveTextContent("Local indisponible");
});

it("un tour vocal que le panneau n'a pas demandé est adopté : le texte dit s'affiche, puis la réponse", () => {
  render(<IrisReply />);
  ev({ type: "reply", turn: 5, segments: [{ t: "Text", text: "perdu" }], done: true }); // sans échange : ignoré
  expect(screen.queryByText("perdu")).toBeNull();
  ev({ type: "user", turn: 5, text: "quelle heure est-il" });
  expect(screen.getByText("quelle heure est-il")).toBeInTheDocument();
  ev({ type: "reply", turn: 5, segments: [{ t: "Text", text: "Il est midi." }], done: true });
  expect(screen.getByText("Il est midi.")).toBeInTheDocument();
  ev({ type: "user", turn: 4, text: "périmé" }); // plus ancien : ignoré
  expect(screen.queryByText("périmé")).toBeNull();
});
