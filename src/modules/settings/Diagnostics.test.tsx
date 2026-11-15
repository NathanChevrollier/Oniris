import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { Diagnostics, checks, report, diagIpc, type Inputs } from "./Diagnostics";
import { SetCtx } from "./controls";
import { DEFAULT_SETTINGS, type Settings } from "../../lib/settings";
import { irisIpc } from "../iris/api";
import { voiceIpc } from "../voice/api";
import { accountsIpc } from "../accounts/api";
import { updateIpc, type UpdateStatus } from "./update";

const upd: UpdateStatus = { current: "1.2.3", pending: null, checking: false, last_error: null, updated: null, state: "idle", last_check_ms: null };
const good: Inputs = {
  iris: { local: "ready", model: "qwen3:14b", claude: { kind: "missing" }, recommended: "qwen3:14b", vram_mb: 8000, ollama_installed: true },
  voice: { installed: true, missing: [], bytes_missing: 0 }, mics: ["Micro de Nathan"], wake: "à l'écoute", azureKey: true,
  accounts: [
    { id: 1, provider: "google", label: "nathan.perso@gmail.com", color: "#fff", status: "ok" },
    { id: 2, provider: "spotify", label: "Nathan", color: "#fff", status: "ok" },
  ],
  update: upd, index: { state: "ready", count: 123456 },
};
const noop = () => {};
const acts = { reload: noop, installOllama: noop, pullModel: noop, installVoice: noop, checkUpdate: noop, enableAutostart: noop, accounts: noop, go: noop };
const level = (list: ReturnType<typeof checks>, id: string) => list.find((c) => c.id === id)!;

it("tout va bien : vert partout, rien à réparer sauf vérifier les mises à jour", () => {
  const list = checks(good, { ...DEFAULT_SETTINGS, voice_wake: true, voice_azure: true }, acts);
  expect(list.map((c) => c.level)).toEqual(Array(list.length).fill("ok"));
  expect(level(list, "index").text).toContain("123\u202f456");
});

it("états dégradés : orange ou rouge avec l'action qui répare", () => {
  const s: Settings = { ...DEFAULT_SETTINGS, voice_wake: true, voice_azure: true, autostart: false };
  const bad: Inputs = {
    ...good, iris: { ...good.iris!, ollama_installed: false }, mics: [], wake: "micro indisponible", azureKey: false,
    accounts: [{ ...good.accounts![0], status: "reauth" }], index: { state: "indexing", done: 10 }, update: { ...upd, last_error: "GitHub ne répond pas." },
  };
  const list = checks(bad, s, acts);
  expect(level(list, "ollama")).toMatchObject({ level: "bad", action: { label: "Installer" } });
  expect(level(list, "voice")).toMatchObject({ level: "bad" });
  expect(level(list, "wake")).toMatchObject({ level: "bad" });
  expect(level(list, "azure")).toMatchObject({ level: "bad", action: { label: "Ouvrir" } });
  expect(level(list, "google")).toMatchObject({ level: "warn", action: { label: "Ouvrir Comptes" } });
  expect(level(list, "spotify")).toMatchObject({ level: "off" });
  expect(level(list, "index").level).toBe("warn");
  expect(level(list, "update").level).toBe("warn");
  expect(level(list, "autostart")).toMatchObject({ level: "warn", action: { label: "Activer" } });
  expect(level(checks({ ...good, iris: { ...good.iris!, local: "no_model" } }, s, acts), "ollama").action?.label).toBe("Télécharger");
  // Lecture échouée : orange, jamais une erreur.
  expect(level(checks({ ...good, iris: null }, s, acts), "ollama")).toMatchObject({ level: "warn", text: "État illisible pour le moment." });
});

it("le rapport ne contient ni e-mail, ni nom de compte, ni nom de micro, ni chemin", () => {
  const text = report(checks(good, { ...DEFAULT_SETTINGS, voice_azure: true }, acts), "1.2.3");
  expect(text).toMatch(/^Diagnostic Oniris 1\.2\.3/);
  expect(text).toContain("- Ollama : OK");
  for (const secret of ["@", "nathan", "Nathan", "Micro de", "C:\\", "Users"]) expect(text).not.toContain(secret);
});

describe("écran", () => {
  beforeEach(() => {
    vi.spyOn(irisIpc, "status").mockResolvedValue(good.iris!);
    vi.spyOn(voiceIpc, "status").mockResolvedValue(good.voice!);
    vi.spyOn(voiceIpc, "devices").mockResolvedValue(good.mics!);
    vi.spyOn(voiceIpc, "wakeState").mockResolvedValue("désactivé");
    vi.spyOn(voiceIpc, "azureKeyStatus").mockResolvedValue(false);
    vi.spyOn(accountsIpc, "list").mockResolvedValue(good.accounts!);
    vi.spyOn(updateIpc, "status").mockResolvedValue(upd);
    vi.spyOn(diagIpc, "index").mockRejectedValue("indisponible");
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it("une ligne par composant, « Réparer » relit, « Copier le rapport » copie un texte sans donnée perso", async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText: write } });
    const save = vi.fn().mockResolvedValue(true);
    const accounts = vi.fn();
    render(<SetCtx.Provider value={{ s: { ...DEFAULT_SETTINGS, autostart: false }, save }}><Diagnostics onAccounts={accounts} go={noop} /></SetCtx.Provider>);
    await act(async () => {});
    expect(screen.getByText("Ollama").closest(".diag-row")).toHaveAttribute("data-level", "ok");
    expect(screen.getByText("Index de recherche").closest(".diag-row")).toHaveAttribute("data-level", "warn");
    fireEvent.click(screen.getByRole("button", { name: "Activer" }));
    expect(save).toHaveBeenCalledWith({ autostart: true });
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "Copier le rapport" }));
    await act(async () => {});
    const text = write.mock.calls[0][0] as string;
    expect(text).toContain("Démarrage avec Windows");
    expect(text).not.toContain("@");
    expect(screen.getByRole("status")).toHaveTextContent("Rapport copié.");
  });
});
