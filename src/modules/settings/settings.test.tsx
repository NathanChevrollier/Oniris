import { render, screen, cleanup, fireEvent, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach } from "vitest";
import { SettingsSheet, hotkeyFromEvent } from "./SettingsSheet";
import { ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS } from "../../lib/settings";
import { projectsIpc, type Project } from "../projects/api";

vi.mock("../../lib/settings", async (orig) => ({ ...(await orig<typeof import("../../lib/settings")>()), useSettings: () => (globalThis as { __s?: typeof DEFAULT_SETTINGS }).__s ?? DEFAULT_SETTINGS }));

const hidden = (): Project => ({ path: "C:/p/Vieux", name: "Vieux", last_activity_ms: 0, branch: null, dirty: 0, activity_14d: [], plan: null, last_session: null, pinned: false, hidden: true });
const setup = () => {
  vi.spyOn(projectsIpc, "listHidden").mockResolvedValue([]);
  return render(<SettingsSheet open onClose={() => {}} onGo={() => {}} />);
};
afterEach(() => { cleanup(); vi.restoreAllMocks(); delete (globalThis as { __s?: unknown }).__s; });

it("chaque section n'enregistre que ses champs, sur des réglages relus", async () => {
  const fresh = { ...DEFAULT_SETTINGS, user_name: "Frais", autostart: false };
  vi.spyOn(ipc, "getSettings").mockResolvedValue(fresh);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  setup();
  const city = screen.getByLabelText("Ville de la météo");
  await userEvent.type(city, "Lyon");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer · Journée" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith({ ...fresh, weather_city: "Lyon" }));
});

it("une erreur de Rust s'affiche sous la section", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  vi.spyOn(ipc, "saveSettings").mockRejectedValue("la relève doit être de 2 à 5 minutes");
  setup();
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer · Mails" }));
  expect((await screen.findByRole("alert")).textContent).toBe("la relève doit être de 2 à 5 minutes");
});

it("enregistrer « Journée » n'efface pas la saisie non enregistrée de « Général »", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  const { rerender } = setup();
  const name = screen.getByLabelText("Prénom") as HTMLInputElement;
  await userEvent.type(name, "Brouillon");
  await userEvent.type(screen.getByLabelText("Ville de la météo"), "Lyon");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer · Journée" }));
  (globalThis as { __s?: unknown }).__s = { ...DEFAULT_SETTINGS, weather_city: "Lyon" };
  rerender(<SettingsSheet open onClose={() => {}} onGo={() => {}} />);
  expect(name.value).toBe("Brouillon");
  expect((screen.getByLabelText("Ville de la météo") as HTMLInputElement).value).toBe("Lyon");
});

it("capture du raccourci : Ctrl+Maj+F5 donne {ctrl, shift, key: F5}", async () => {
  expect(hotkeyFromEvent({ code: "F5", altKey: false, ctrlKey: true, shiftKey: true, metaKey: false }))
    .toEqual({ alt: false, ctrl: true, shift: true, win: false, key: "F5" });
  expect(hotkeyFromEvent({ code: "ControlLeft", altKey: false, ctrlKey: true, shiftKey: false, metaKey: false })).toBeNull();
  const fresh = DEFAULT_SETTINGS;
  vi.spyOn(ipc, "getSettings").mockResolvedValue(fresh);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  setup();
  fireEvent.keyDown(screen.getByLabelText("Raccourci"), { code: "F5", key: "F5", ctrlKey: true, shiftKey: true });
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer · Raccourci" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ hotkey: { alt: false, ctrl: true, shift: true, win: false, key: "F5" } })));
});

it("un raccourci refusé par Rust affiche son message", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  vi.spyOn(ipc, "saveSettings").mockRejectedValue("raccourci réservé");
  setup();
  fireEvent.keyDown(screen.getByLabelText("Raccourci"), { code: "KeyA", key: "a", ctrlKey: true });
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer · Raccourci" }));
  expect((await screen.findByRole("alert")).textContent).toBe("raccourci réservé");
});

it("« Ajouter un dossier » ouvre le sélecteur et l'enregistre dans la liste", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  vi.spyOn(projectsIpc, "browse").mockResolvedValue("C:/Docs");
  setup();
  await userEvent.click(screen.getByRole("button", { name: "Ajouter un dossier : Dossiers de recherche" }));
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer · Recherche et projets" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ search_roots: ["C:/Docs"], projects_roots: [] })));
});

it("ré-afficher un projet masqué appelle projects_hide(…, false)", async () => {
  vi.spyOn(projectsIpc, "listHidden").mockResolvedValueOnce([hidden()]).mockResolvedValue([]);
  const hide = vi.spyOn(projectsIpc, "hide").mockResolvedValue();
  render(<SettingsSheet open onClose={() => {}} onGo={() => {}} />);
  await userEvent.click(await screen.findByRole("button", { name: "Ré-afficher Vieux" }));
  expect(hide).toHaveBeenCalledWith("C:/p/Vieux", false);
  await waitFor(() => expect(screen.queryByRole("button", { name: "Ré-afficher Vieux" })).toBeNull());
});

it("les liens ouvrent les feuilles IRIS et Comptes", async () => {
  vi.spyOn(projectsIpc, "listHidden").mockResolvedValue([]);
  const go = vi.fn();
  render(<SettingsSheet open onClose={() => {}} onGo={go} />);
  await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Ouvrir IRIS" }));
  expect(go).toHaveBeenCalledWith("iris");
});

it("une valeur hors bornes part quand même à Rust, qui seul formule le refus", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockRejectedValue("la relève doit être de 2 à 5 minutes");
  setup();
  const poll = screen.getByLabelText("Relève (minutes)");
  await userEvent.clear(poll);
  await userEvent.type(poll, "9");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer · Mails" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ mail_poll_minutes: 9 })));
  expect((await screen.findByRole("alert")).textContent).toBe("la relève doit être de 2 à 5 minutes");
});
