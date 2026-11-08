import { render, screen, cleanup, act, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { IrisOrb } from "../iris/IrisReply";
import { IrisSheet } from "../iris/IrisSheet";
import { irisIpc } from "../iris/api";
import { ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS } from "../../lib/settings";
import { usePanelStore } from "../../panel/store";
import { PillToast, TOAST_MS } from "./PillToast";
import { voiceIpc, type InstallEvent, type VoiceEvent, type VoiceStatus } from "./api";

const handlers: Record<string, (p: unknown) => void> = {};
vi.mock("../../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../../lib/ipc")>()),
  listen: (event: string, cb: (e: { payload: unknown }) => void) => { handlers[event] = (payload) => cb({ payload }); return Promise.resolve(() => {}); },
}));
const voiceCb = (e: VoiceEvent) => handlers["oniris://voice"](e);
const toastCb = (text: string) => handlers["oniris://toast"]({ text });
const installCb = (e: InstallEvent) => handlers["oniris://voice-install"](e);

const missing: VoiceStatus = { installed: false, missing: ["a"], bytes_missing: 276_000_000 };
beforeEach(() => {
  usePanelStore.setState({ open: true });
  vi.spyOn(voiceIpc, "status").mockResolvedValue(missing);
  vi.spyOn(voiceIpc, "devices").mockResolvedValue(["Micro USB"]);
  vi.spyOn(irisIpc, "status").mockResolvedValue({ local: "ready", model: "m", claude: { kind: "missing" } });
  vi.spyOn(irisIpc, "memories").mockResolvedValue([]);
  vi.spyOn(irisIpc, "history").mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });

it("les événements de voix changent l'état de la sphère et --level", () => {
  const { container } = render(<IrisOrb size={30} />);
  const orb = () => container.querySelector<HTMLElement>(".orb")!;
  expect(orb().dataset.state).toBe("idle");
  act(() => voiceCb({ phase: "listening", level: 0.4 }));
  expect(orb().dataset.state).toBe("listening");
  expect(orb().style.getPropertyValue("--level")).toBe("0.4");
  act(() => voiceCb({ phase: "transcribing" }));
  expect(orb().dataset.state).toBe("thinking");
  expect(orb().style.getPropertyValue("--level")).toBe("0");
  act(() => voiceCb({ phase: "speaking" }));
  expect(orb().dataset.state).toBe("speaking");
  act(() => voiceCb({ phase: "idle" }));
  expect(orb().dataset.state).toBe("idle");
});

it("le toast s'affiche en texte puis disparaît au bout de 3 s ; un nouveau remplace l'ancien", () => {
  vi.useFakeTimers();
  const { container } = render(<PillToast />);
  const live = screen.getByRole("status");
  expect(live.getAttribute("aria-live")).toBe("polite");
  act(() => toastCb("<b>Salut</b>"));
  expect(live.textContent).toBe("<b>Salut</b>");
  expect(container.querySelector("b")).toBeNull();
  act(() => { vi.advanceTimersByTime(TOAST_MS - 100); });
  act(() => toastCb("Deux"));
  act(() => { vi.advanceTimersByTime(TOAST_MS - 100); });
  expect(live.textContent).toBe("Deux");
  act(() => { vi.advanceTimersByTime(100); });
  expect(live.textContent).toBe("");
});

it("installation : taille, progression par fichier, puis « Installée »", async () => {
  let finish = () => {};
  const install = vi.spyOn(voiceIpc, "install").mockReturnValue(new Promise<void>((r) => { finish = r; }));
  render(<IrisSheet open onClose={() => {}} />);
  expect(await screen.findByText("À installer · 276 Mo")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Installer la voix" }));
  expect(install).toHaveBeenCalled();
  act(() => installCb({ key: "piper", done: 50, total: 200 }));
  expect(screen.getByText("piper : 25 %")).toBeTruthy();
  vi.mocked(voiceIpc.status).mockResolvedValue({ installed: true, missing: [], bytes_missing: 0 });
  await act(async () => finish());
  expect(await screen.findByText("Installée")).toBeTruthy();
});

it("une erreur d'installation (hash) est affichée", async () => {
  vi.spyOn(voiceIpc, "install").mockRejectedValue("Fichier corrompu : empreinte invalide.");
  render(<IrisSheet open onClose={() => {}} />);
  await userEvent.click(await screen.findByRole("button", { name: "Installer la voix" }));
  expect((await screen.findByRole("alert")).textContent).toBe("Fichier corrompu : empreinte invalide.");
  act(() => installCb({ error: "Empreinte invalide." }));
  expect((await screen.findByRole("alert")).textContent).toBe("Empreinte invalide.");
});

it("les réglages de la voix sont enregistrés sur des réglages relus", async () => {
  const fresh = { ...DEFAULT_SETTINGS, user_name: "Frais" };
  vi.spyOn(ipc, "getSettings").mockResolvedValue(fresh);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  render(<IrisSheet open onClose={() => {}} />);
  expect(screen.getByText("Ta voix reste sur ton PC : rien n'est enregistré ni envoyé.")).toBeTruthy();
  expect((screen.getByLabelText(/Mot d'activation : bientôt/) as HTMLInputElement).disabled).toBe(true);
  await userEvent.selectOptions(screen.getByLabelText("Transcription"), "turbo");
  await userEvent.selectOptions(screen.getByLabelText("Micro"), "Micro USB");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer la voix" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith({ ...fresh, voice_stt_model: "turbo", voice_input_device: "Micro USB" }));
});
