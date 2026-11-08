import { render, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach } from "vitest";
import { Welcome } from "./Welcome";
import { onboardingIpc, type Onboarding } from "./api";
import { ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS } from "../../lib/settings";

vi.mock("../../lib/settings", async (orig) => ({ ...(await orig<typeof import("../../lib/settings")>()), useSettings: () => DEFAULT_SETTINGS }));

const start = (s: Partial<Onboarding> = {}, suggested_root: string | null = null) => {
  const state: Onboarding = { step: 0, done: false, autostart_consent: null, ...s };
  vi.spyOn(onboardingIpc, "get").mockImplementation(async () => ({ ...state, suggested_root }));
  const set = vi.spyOn(onboardingIpc, "set").mockImplementation(async (o) => { Object.assign(state, o); });
  const save = vi.spyOn(ipc, "saveSettings").mockResolvedValue(undefined);
  vi.spyOn(ipc, "getSettings").mockResolvedValue(DEFAULT_SETTINGS);
  return { set, save, state };
};
const click = (name: string) => userEvent.click(screen.getByRole("button", { name }));
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("parcours complet : prénom, consentement, dossiers, suite, fin", async () => {
  const { set, save } = start({}, "C:\\Users\\moi\\Desktop\\Utilitaires\\Projets");
  render(<Welcome onGo={() => {}} />);
  await screen.findByText("Bonjour ! Comment dois-je t'appeler ?");
  await userEvent.type(screen.getByLabelText("Prénom"), "Nathan");
  await click("Continuer");
  await waitFor(() => expect(save).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, user_name: "Nathan" }));
  await screen.findByText(/Lancer Oniris avec Windows/);
  await click("Oui");
  expect(set).toHaveBeenLastCalledWith({ step: 3, done: false, autostart_consent: true });
  expect(screen.getByText("C:\\Users\\moi\\Desktop\\Utilitaires\\Projets")).toBeTruthy(); // proposition
  await click("Continuer");
  await waitFor(() => expect(save).toHaveBeenLastCalledWith({ ...DEFAULT_SETTINGS, projects_roots: ["C:\\Users\\moi\\Desktop\\Utilitaires\\Projets"], search_roots: [] }));
  await screen.findByText(/Pour aller plus loin/);
  await click("Continuer");
  await screen.findByText(/C'est prêt/);
  expect(screen.getByText("Espace")).toBeTruthy();
  await click("Terminer");
  expect(set).toHaveBeenLastCalledWith({ step: 5, done: true, autostart_consent: true });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("reprise à l'étape 3 après remontage", async () => {
  start({}, null);
  const first = render(<Welcome onGo={() => {}} />);
  await userEvent.click(await screen.findByRole("button", { name: "Passer" }));
  await click("Non");
  await screen.findByText(/Où sont tes projets/);
  first.unmount();
  render(<Welcome onGo={() => {}} />);
  await screen.findByText(/Où sont tes projets/);
});

it("« Passer » ne saute jamais l'écran 2, Échap non plus", async () => {
  start({ step: 2 });
  render(<Welcome onGo={() => {}} />);
  await screen.findByText(/Lancer Oniris avec Windows/);
  expect(screen.queryByRole("button", { name: "Passer" })).toBeNull();
  await userEvent.keyboard("{Escape}");
  expect(screen.getByText(/Lancer Oniris avec Windows/)).toBeTruthy();
});

it("« Non » enregistre le refus, rien d'autre", async () => {
  const { set } = start({ step: 2 });
  render(<Welcome onGo={() => {}} />);
  await screen.findByText(/Lancer Oniris avec Windows/);
  await click("Non");
  expect(set).toHaveBeenCalledWith({ step: 3, done: false, autostart_consent: false });
});

it("l'écran « Pour aller plus loin » mène aux feuilles", async () => {
  start({ step: 4 });
  const onGo = vi.fn();
  render(<Welcome onGo={onGo} />);
  await screen.findByText(/Pour aller plus loin/);
  await click("IRIS (Ollama)");
  await click("Comptes (mail, agenda, Spotify)");
  expect(onGo.mock.calls).toEqual([["iris"], ["accounts"]]);
});

it("un onboarding terminé n'affiche rien", async () => {
  start({ done: true, step: 5 });
  const onActive = vi.fn();
  render(<Welcome onGo={() => {}} onActive={onActive} />);
  await waitFor(() => expect(onActive).toHaveBeenCalledWith(false));
  expect(screen.queryByRole("dialog")).toBeNull();
});
