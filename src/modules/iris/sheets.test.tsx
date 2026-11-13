import { render, screen, cleanup, act, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { useCallback, useState } from "react";
import { SettingsSheet, type SettingsCat } from "../settings/SettingsSheet";
import { JournalSheet } from "./JournalSheet";
import { irisIpc, type IrisStatus, type PullEvent } from "./api";
import { Header, focusHeaderButton } from "../../panel/Header";
import { accountsIpc } from "../accounts/api";
import { ipc, toolIpc, type JournalEntry } from "../../lib/ipc";
import { DEFAULT_SETTINGS } from "../../lib/settings";
import { usePanelStore } from "../../panel/store";

let pullCb: (e: PullEvent) => void = () => {};
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), onIrisPull: (cb: (e: PullEvent) => void) => { pullCb = cb; return Promise.resolve(() => {}); } }));
vi.mock("../../lib/settings", async (orig) => ({ ...(await orig<typeof import("../../lib/settings")>()), useSettings: () => (globalThis as { __s?: typeof DEFAULT_SETTINGS }).__s ?? DEFAULT_SETTINGS }));

const status = (local: IrisStatus["local"]): IrisStatus => ({ local, model: "qwen3:8b", claude: { kind: "missing" }, recommended: "qwen3:8b", vram_mb: 8192, ollama_installed: true });
beforeEach(() => {
  usePanelStore.setState({ open: true });
  vi.spyOn(irisIpc, "status").mockResolvedValue(status("ready"));
  vi.spyOn(irisIpc, "memories").mockResolvedValue([]);
  vi.spyOn(irisIpc, "history").mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const Sheet = ({ at = "iris" }: { at?: SettingsCat }) => <SettingsSheet open section={at} onClose={() => {}} onAccounts={() => {}} />;

it("modèle absent : téléchargement, puis la progression s'affiche", async () => {
  vi.mocked(irisIpc.status).mockResolvedValue(status("no_model"));
  let finish = () => {};
  const pull = vi.spyOn(irisIpc, "pullModel").mockReturnValue(new Promise<void>((r) => { finish = r; }));
  render(<Sheet />);
  await userEvent.click(await screen.findByRole("button", { name: "Télécharger le modèle" }));
  expect(pull).toHaveBeenCalled();
  act(() => pullCb({ done: 50, total: 200, error: null }));
  expect(screen.getByText("Téléchargement : 25 %")).toBeTruthy();
  vi.mocked(irisIpc.status).mockResolvedValue(status("ready"));
  await act(async () => finish());
  expect(await screen.findByText(/Prêt/)).toBeTruthy();
});

it("Ollama absent : installation par winget, et en cas d'échec le lien de téléchargement", async () => {
  vi.mocked(irisIpc.status).mockResolvedValue({ ...status("down"), ollama_installed: false });
  const install = vi.spyOn(irisIpc, "installOllama").mockRejectedValue("installation impossible");
  const open = vi.spyOn(accountsIpc, "openGuide").mockResolvedValue();
  render(<Sheet />);
  await userEvent.click(await screen.findByRole("button", { name: "Installer Ollama" }));
  expect(install).toHaveBeenCalled();
  await userEvent.click(await screen.findByRole("button", { name: /Télécharger Ollama/ }));
  expect(open).toHaveBeenCalledWith("https://ollama.com/download");
});

it("modèle plus adapté à la carte graphique : « Utiliser » l'enregistre", async () => {
  vi.mocked(irisIpc.status).mockResolvedValue({ ...status("ready"), recommended: "qwen3:14b", vram_mb: 16_384 });
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue();
  render(<Sheet />);
  expect(await screen.findByText(/\(16 Go\)/)).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Utiliser" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ iris_local_model: "qwen3:14b" })));
});

it("la liste blanche est enregistrée à la perte du focus, sur des réglages relus, et l'erreur de Rust s'affiche", async () => {
  const fresh = { ...DEFAULT_SETTINGS, user_name: "Frais" };
  vi.spyOn(ipc, "getSettings").mockResolvedValue(fresh);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValueOnce(undefined).mockRejectedValueOnce("Commande trop longue.");
  render(<Sheet />);
  const box = screen.getByLabelText(/Commandes autorisées/);
  await userEvent.clear(box);
  await userEvent.type(box, "git log{Enter}Get-Date");
  expect(save).not.toHaveBeenCalled();
  await userEvent.tab();
  await waitFor(() => expect(save).toHaveBeenCalledWith({ ...fresh, iris_shell_allowlist: ["git log", "Get-Date"] }));
  await userEvent.type(box, "{Enter}x");
  await userEvent.tab();
  expect((await screen.findByRole("alert")).textContent).toBe("Commande trop longue.");
});

it("Claude sur demande par défaut ; cocher l'appel automatique l'enregistre", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  render(<Sheet />);
  const box = screen.getByRole("checkbox", { name: /IRIS peut appeler Claude d'elle-même/ });
  expect(box).not.toBeChecked();
  await userEvent.click(box);
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ iris_claude_auto: true })));
});

it("« Rétablir » remet la personnalité par défaut", async () => {
  render(<Sheet />);
  const area = screen.getByLabelText("Personnalité d'IRIS") as HTMLTextAreaElement;
  await userEvent.clear(area);
  await userEvent.type(area, "x");
  await userEvent.click(screen.getByRole("button", { name: "Rétablir" }));
  expect(area.value).toBe(DEFAULT_SETTINGS.iris_personality);
});

it("supprime une mémoire", async () => {
  vi.mocked(irisIpc.memories).mockResolvedValue([{ id: 7, text: "aime le thé", created_ms: 0 }]);
  const del = vi.spyOn(irisIpc, "memoryDelete").mockResolvedValue(null);
  render(<Sheet at="memory" />);
  await userEvent.click(await screen.findByRole("button", { name: "Oublier : aime le thé" }));
  expect(del).toHaveBeenCalledWith(7);
});

it("« Tout effacer » demande confirmation et garde les préférences", async () => {
  const clear = vi.spyOn(irisIpc, "historyClear").mockResolvedValue();
  render(<Sheet at="memory" />);
  await userEvent.click(screen.getByRole("button", { name: "Tout effacer" }));
  expect(clear).not.toHaveBeenCalled();
  expect(screen.getByText(/préférences retenues/)).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Confirmer l'effacement" }));
  expect(clear).toHaveBeenCalled();
});

const entry = (over: Partial<JournalEntry>): JournalEntry => ({ id: 1, ts_ms: 0, tool: "mail_send", args: { to: "<b>marc</b>" }, origin: { Assistant: { brain: "Local" } }, provenance: { Untrusted: { source: "mail de Marc" } }, status: "done", result: { ok: true }, undoable: true, ...over });

it("journal : provenance lisible, « Annuler » appelle undo puis recharge", async () => {
  const list = vi.spyOn(toolIpc, "journal").mockResolvedValue([entry({}), entry({ id: 2, origin: "Mcp", provenance: "User", undoable: false, result: { t: "<b>x</b>".repeat(40) } })]);
  const undo = vi.spyOn(toolIpc, "undo").mockResolvedValue();
  render(<JournalSheet open onClose={() => {}} />);
  expect(await screen.findByText("contenu non fiable : mail de Marc")).toBeTruthy();
  expect(screen.getByText(/IRIS local/)).toBeTruthy();
  expect(screen.getByText(/MCP/)).toBeTruthy();
  expect(screen.getAllByRole("button", { name: /Annuler/ })).toHaveLength(1);
  expect(screen.getByText(/…$/).textContent!.length).toBeLessThanOrEqual(121);
  await userEvent.click(screen.getByRole("button", { name: "Annuler mail_send" }));
  expect(undo).toHaveBeenCalledWith(1);
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
});

it("le focus revient au bouton d'en-tête à la fermeture", async () => {
  vi.spyOn(toolIpc, "journal").mockResolvedValue([]);
  function Harness() {
    const [o, setO] = useState(false);
    const close = useCallback(() => { setO(false); focusHeaderButton("journal"); }, []);
    return <><div inert={o}><Header name="N" onJournal={() => setO(true)} /></div><JournalSheet open={o} onClose={close} /></>;
  }
  render(<Harness />);
  const btn = screen.getByRole("button", { name: "Journal" });
  await userEvent.click(btn);
  await userEvent.click(await screen.findByRole("button", { name: "Fermer le journal" }));
  await waitFor(() => expect(document.activeElement).toBe(btn));
});

it("journal : les arguments et le statut localisé s'affichent en texte brut", async () => {
  vi.spyOn(toolIpc, "journal").mockResolvedValue([entry({})]);
  const { container } = render(<JournalSheet open onClose={() => {}} />);
  expect(await screen.findByText('{"to":"<b>marc</b>"}')).toBeTruthy();
  expect(container.querySelector("b")).toBeNull();
  expect(screen.getByText(/· fait$/)).toBeTruthy();
});

it("échecs : chargement et suppression affichent une erreur", async () => {
  vi.mocked(irisIpc.history).mockRejectedValue("x");
  vi.mocked(irisIpc.memories).mockResolvedValue([{ id: 7, text: "aime le thé", created_ms: 0 }]);
  vi.spyOn(irisIpc, "memoryDelete").mockRejectedValue("Suppression refusée.");
  render(<Sheet at="memory" />);
  expect(await screen.findByText("Impossible de charger l'historique.")).toBeTruthy();
  await userEvent.click(await screen.findByRole("button", { name: "Oublier : aime le thé" }));
  expect(await screen.findByText("Suppression refusée.")).toBeTruthy();
});

afterEach(() => { delete (globalThis as { __s?: unknown }).__s; });

it("un réglage modifié ailleurs ne réinitialise pas le texte non enregistré de la personnalité", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const { rerender } = render(<Sheet />);
  const area = screen.getByLabelText("Personnalité d'IRIS") as HTMLTextAreaElement;
  await userEvent.clear(area);
  await userEvent.type(area, "brouillon");
  (globalThis as { __s?: unknown }).__s = { ...DEFAULT_SETTINGS, iris_shell_allowlist: ["git log"] };
  rerender(<Sheet />);
  expect(area.value).toBe("brouillon");
  expect((screen.getByLabelText(/Commandes autorisées/) as HTMLTextAreaElement).value).toBe("git log");
});

it("le bouton d'en-tête « IRIS » ouvre les réglages sur IRIS, le focus y revient à la fermeture", async () => {
  function Harness() {
    const [at, setAt] = useState<SettingsCat | null>(null);
    const close = useCallback(() => { setAt(null); focusHeaderButton("iris"); }, []);
    return <><div inert={at !== null}><Header name="N" onIris={() => setAt("iris")} onSettings={() => setAt("general")} /></div>
      <SettingsSheet open={at !== null} section={at ?? "general"} onClose={close} onAccounts={() => {}} /></>;
  }
  render(<Harness />);
  const btn = screen.getByRole("button", { name: "IRIS" });
  await userEvent.click(btn);
  expect(screen.getByRole("dialog", { name: "Réglages" })).toBeTruthy();
  expect(screen.getByRole("tab", { name: "IRIS" })).toHaveAttribute("aria-selected", "true");
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(document.activeElement).toBe(btn));
  await userEvent.click(screen.getByRole("button", { name: "Réglages" }));
  expect(screen.getByRole("tab", { name: "Général" })).toHaveAttribute("aria-selected", "true");
});
