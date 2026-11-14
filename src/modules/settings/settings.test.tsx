import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach } from "vitest";
import { SettingsSheet, hotkeyFromEvent, type SettingsCat } from "./SettingsSheet";
import { ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS } from "../../lib/settings";
import { updateIpc } from "./update";
import { projectsIpc, type Project } from "../projects/api";

vi.mock("../../lib/settings", async (orig) => ({ ...(await orig<typeof import("../../lib/settings")>()), useSettings: () => (globalThis as { __s?: typeof DEFAULT_SETTINGS }).__s ?? DEFAULT_SETTINGS }));

const hidden = (): Project => ({ path: "C:/p/Vieux", name: "Vieux", last_activity_ms: 0, branch: null, dirty: 0, activity_14d: [], plan: null, last_session: null, pinned: false, hidden: true });
const sheet = (section: SettingsCat = "general", onAccounts = () => {}) => <SettingsSheet open section={section} onClose={() => {}} onAccounts={onAccounts} />;
const setup = (section?: SettingsCat) => {
  vi.spyOn(projectsIpc, "listHidden").mockResolvedValue([]);
  vi.spyOn(updateIpc, "status").mockResolvedValue({ current: "1.2.0", pending: null, checking: false, last_error: null, updated: null, state: "idle" });
  return render(sheet(section));
};
afterEach(() => { cleanup(); vi.restoreAllMocks(); delete (globalThis as { __s?: unknown }).__s; });

it("la barre latérale change de catégorie au clic et aux flèches", async () => {
  setup();
  expect(screen.getByRole("heading", { name: "Général" })).toBeTruthy();
  await userEvent.click(screen.getByRole("tab", { name: "Journée" }));
  expect(screen.getByRole("tab", { name: "Journée" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("heading", { name: "Journée" })).toBeTruthy();
  expect(screen.getByLabelText("Ville de la météo")).toBeTruthy();
  expect(screen.queryByLabelText("Prénom")).toBeNull();
  fireEvent.keyDown(screen.getByRole("tab", { name: "Journée" }), { key: "ArrowDown" });
  expect(screen.getByRole("tab", { name: "IRIS" })).toHaveFocus();
  expect(screen.getByRole("heading", { name: "IRIS" })).toBeTruthy();
  fireEvent.keyDown(screen.getByRole("tab", { name: "IRIS" }), { key: "Home" });
  expect(screen.getByRole("tab", { name: "Général" })).toHaveAttribute("aria-selected", "true");
});

it("« Comptes » dans la barre latérale ouvre la feuille Comptes", async () => {
  vi.spyOn(projectsIpc, "listHidden").mockResolvedValue([]);
  const go = vi.fn();
  render(sheet("general", go));
  await userEvent.click(screen.getByRole("button", { name: "Comptes" }));
  expect(go).toHaveBeenCalled();
});

it("un interrupteur s'enregistre tout de suite, sur des réglages relus", async () => {
  const fresh = { ...DEFAULT_SETTINGS, user_name: "Frais" };
  vi.spyOn(ipc, "getSettings").mockResolvedValue(fresh);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  setup();
  const box = screen.getByRole("checkbox", { name: "Réduire les animations" });
  await userEvent.click(box);
  expect(box).toBeChecked();
  await waitFor(() => expect(save).toHaveBeenCalledWith({ ...fresh, reduce_motion: true }));
  // Le curseur « Intensité du verre » (<output>) est aussi un « status » : on vise l'annonce elle-même.
  expect(await screen.findByText("Enregistré")).toHaveAttribute("role", "status");
});

it("un interrupteur refusé par Rust revient en arrière et affiche l'erreur", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  vi.spyOn(ipc, "saveSettings").mockRejectedValue("démarrage refusé");
  setup();
  const box = screen.getByRole("checkbox", { name: "Lancer Oniris au démarrage de Windows" });
  await userEvent.click(box);
  expect((await screen.findByRole("alert")).textContent).toBe("démarrage refusé");
  expect(box).toBeChecked();
});

it("un champ texte s'enregistre à la perte du focus, sur des réglages relus", async () => {
  const fresh = { ...DEFAULT_SETTINGS, user_name: "Frais", autostart: false };
  vi.spyOn(ipc, "getSettings").mockResolvedValue(fresh);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  setup("day");
  await userEvent.type(screen.getByLabelText("Ville de la météo"), "Lyon");
  expect(save).not.toHaveBeenCalled();
  await userEvent.tab();
  await waitFor(() => expect(save).toHaveBeenCalledWith({ ...fresh, weather_city: "Lyon" }));
});

it("une valeur hors bornes part quand même à Rust (Entrée), qui seul formule le refus", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockRejectedValue("la relève doit être de 2 à 5 minutes");
  setup("day");
  const poll = screen.getByLabelText("Relève des mails");
  await userEvent.clear(poll);
  await userEvent.type(poll, "9{Enter}");
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ mail_poll_minutes: 9 })));
  expect((await screen.findByRole("alert")).textContent).toBe("la relève doit être de 2 à 5 minutes");
  expect((poll as HTMLInputElement).value).toBe("9");
});

it("un brouillon n'est pas effacé quand un autre réglage change", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const { rerender } = setup();
  const name = screen.getByLabelText("Prénom") as HTMLInputElement;
  await userEvent.type(name, "Brouillon");
  (globalThis as { __s?: unknown }).__s = { ...DEFAULT_SETTINGS, weather_city: "Lyon" };
  rerender(sheet());
  expect(name.value).toBe("Brouillon");
});

it("capture du raccourci : Ctrl+Maj+F5 donne {ctrl, shift, key: F5}, enregistré aussitôt", async () => {
  expect(hotkeyFromEvent({ code: "F5", altKey: false, ctrlKey: true, shiftKey: true, metaKey: false }))
    .toEqual({ alt: false, ctrl: true, shift: true, win: false, key: "F5" });
  expect(hotkeyFromEvent({ code: "ControlLeft", altKey: false, ctrlKey: true, shiftKey: false, metaKey: false })).toBeNull();
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  setup("pill");
  fireEvent.keyDown(screen.getByLabelText("Raccourci"), { code: "F5", key: "F5", ctrlKey: true, shiftKey: true });
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ hotkey: { alt: false, ctrl: true, shift: true, win: false, key: "F5" } })));
  expect((screen.getByLabelText("Raccourci") as HTMLInputElement).value).toBe("Ctrl + Maj + F5");
});

it("un raccourci refusé par Rust affiche son message", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  vi.spyOn(ipc, "saveSettings").mockRejectedValue("raccourci réservé");
  setup("pill");
  fireEvent.keyDown(screen.getByLabelText("Raccourci"), { code: "KeyA", key: "a", ctrlKey: true });
  expect((await screen.findByRole("alert")).textContent).toBe("raccourci réservé");
});

it("« Ajouter un dossier » ouvre le sélecteur et l'enregistre dans la liste", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  vi.spyOn(projectsIpc, "browse").mockResolvedValue("C:/Docs");
  setup("search");
  await userEvent.click(screen.getByRole("button", { name: "Ajouter un dossier : Dossiers de recherche" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ search_roots: ["C:/Docs"], projects_roots: [] })));
  expect(screen.getByText("C:/Docs")).toBeTruthy();
});

it("ré-afficher un projet masqué appelle projects_hide(…, false)", async () => {
  vi.spyOn(projectsIpc, "listHidden").mockResolvedValueOnce([hidden()]).mockResolvedValue([]);
  const hide = vi.spyOn(projectsIpc, "hide").mockResolvedValue();
  render(sheet("search"));
  await userEvent.click(await screen.findByRole("button", { name: "Ré-afficher Vieux" }));
  expect(hide).toHaveBeenCalledWith("C:/p/Vieux", false);
  await waitFor(() => expect(screen.queryByRole("button", { name: "Ré-afficher Vieux" })).toBeNull());
});

it("réglages : mises à jour automatiques activées par défaut", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  setup("updates");
  const box = screen.getByLabelText("Mises à jour automatiques") as HTMLInputElement;
  expect(box.checked).toBe(true);
  await userEvent.click(box);
  await waitFor(() => expect(save).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, auto_update: false }));
});

it("réglages : version installée et vérification manuelle", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const check = vi.spyOn(updateIpc, "checkNow").mockResolvedValue({ current: "1.2.0", pending: ["1.2.1", "Corrections diverses"], checking: false, last_error: null, updated: null, state: "ready", version: "1.2.1", notes: "Corrections diverses" });
  setup("updates");
  expect(await screen.findByText("Version 1.2.0")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Vérifier maintenant" }));
  expect(check).toHaveBeenCalled();
  expect(await screen.findByText("Version 1.2.1 prête")).toBeTruthy();
  expect(screen.getByText("Corrections diverses")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Redémarrer pour l'installer" })).toBeTruthy();
});

it("réglages : vérification sans nouveauté ou en échec, le résultat s'affiche", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  const check = vi.spyOn(updateIpc, "checkNow").mockResolvedValue({ current: "1.2.1", pending: null, checking: false, last_error: null, updated: null, state: "idle" });
  setup("updates");
  await userEvent.click(await screen.findByRole("button", { name: "Vérifier maintenant" }));
  expect(await screen.findByText("Oniris est à jour.")).toBeTruthy();
  check.mockResolvedValue({ current: "1.2.1", pending: null, checking: false, last_error: "hors ligne", updated: null, state: "error", message: "hors ligne", version: null });
  await userEvent.click(screen.getByRole("button", { name: "Vérifier maintenant" }));
  expect((await screen.findByRole("alert")).textContent).toContain("hors ligne");
  expect(screen.getByRole("button", { name: "Réessayer" })).toBeTruthy();
});

it("réglages : téléchargement en cours et nouveautés de la version installée", async () => {
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  vi.spyOn(projectsIpc, "listHidden").mockResolvedValue([]);
  vi.spyOn(updateIpc, "status").mockResolvedValue({ current: "1.2.1", pending: null, checking: true, last_error: null,
    updated: { version: "1.2.1", notes: "- Pilule plus claire" }, state: "downloading", version: "1.2.2", percent: 42 });
  render(sheet("updates"));
  expect(await screen.findByText("Téléchargement de la mise à jour 1.2.2… 42 %")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Recherche…" })).toBeDisabled();
  expect(screen.getByText("Nouveautés de la version 1.2.1")).toBeTruthy();
  expect(screen.getByText("- Pilule plus claire")).toBeTruthy();
});
