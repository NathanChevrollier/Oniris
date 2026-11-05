import { render, screen, cleanup, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { AccountsSheet } from "./AccountsSheet";
import { accountsIpc, PALETTE, type AccountView } from "./api";
import { GUIDES, REDIRECT_URI } from "./guide";
import { DEFAULT_SETTINGS, type Settings } from "../../lib/settings";
import { usePanelStore } from "../../panel/store";

let settings: Settings = DEFAULT_SETTINGS;
vi.mock("../../lib/settings", async (orig) => ({ ...(await orig<typeof import("../../lib/settings")>()), useSettings: () => settings }));

afterEach(() => { cleanup(); vi.restoreAllMocks(); settings = DEFAULT_SETTINGS; });
beforeEach(() => { usePanelStore.setState({ open: true }); vi.spyOn(accountsIpc, "introTake").mockResolvedValue(false); });

const acc = (id: number, label: string, status: AccountView["status"] = "ok"): AccountView =>
  ({ id, provider: "google", label, color: PALETTE[id - 1], status });
const sheet = () => render(<AccountsSheet open onClose={() => {}} onAutoOpen={() => {}} />);

it("feuille vide : message et guides visibles", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([]);
  sheet();
  expect(await screen.findByText("Aucun compte connecté pour l'instant.")).toBeTruthy();
  for (const g of GUIDES) expect(screen.getByText(g.title)).toBeTruthy();
});

it("Gmail désactivé sans identifiant client, activé avec", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([]);
  const { rerender } = sheet();
  const gmail = await screen.findByRole("button", { name: "Ajouter un compte Gmail" });
  expect((gmail as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Connecter Spotify" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getAllByRole("button", { name: "Configurer d'abord" })).toHaveLength(2);
  settings = { ...DEFAULT_SETTINGS, google_client_id: "id.apps.googleusercontent.com" };
  rerender(<AccountsSheet open onClose={() => {}} onAutoOpen={() => {}} />);
  expect((screen.getByRole("button", { name: "Ajouter un compte Gmail" }) as HTMLButtonElement).disabled).toBe(false);
});

it("la valeur à copier du guide Spotify est exactement l'URI de redirection", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([]);
  sheet();
  const spotify = (await screen.findByText("Configurer Spotify")).closest("details")!;
  expect(within(spotify).getByText(REDIRECT_URI).textContent).toBe("http://127.0.0.1:47829/callback");
});

it("un compte à reconnecter propose « Reconnecter », qui appelle connect", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([acc(1, "a@gmail.com"), acc(2, "b@gmail.com", "reauth")]);
  const connect = vi.spyOn(accountsIpc, "connect").mockResolvedValue(acc(2, "b@gmail.com"));
  sheet();
  expect(await screen.findByText("À reconnecter")).toBeTruthy();
  const buttons = screen.getAllByRole("button", { name: /Reconnecter/ });
  expect(buttons).toHaveLength(1);
  await userEvent.click(buttons[0]);
  expect(connect).toHaveBeenCalledWith("google");
});

it("une erreur de connexion s'affiche", async () => {
  settings = { ...DEFAULT_SETTINGS, spotify_client_id: "abc" };
  vi.spyOn(accountsIpc, "list").mockResolvedValue([]);
  vi.spyOn(accountsIpc, "connect").mockRejectedValue("Réponse de connexion invalide (state) : réessaie.");
  sheet();
  await userEvent.click(await screen.findByRole("button", { name: "Connecter Spotify" }));
  expect((await screen.findByRole("alert")).textContent).toContain("réessaie");
});

it("changer la couleur appelle setColor", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([acc(1, "a@gmail.com")]);
  const setColor = vi.spyOn(accountsIpc, "setColor").mockResolvedValue();
  sheet();
  await userEvent.click(await screen.findByRole("button", { name: "Couleur de a@gmail.com" }));
  await userEvent.click(screen.getByRole("button", { name: `Couleur ${PALETTE[4]}` }));
  expect(setColor).toHaveBeenCalledWith(1, PALETTE[4]);
});

it("supprimer un compte appelle remove", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([acc(1, "a@gmail.com")]);
  const remove = vi.spyOn(accountsIpc, "remove").mockResolvedValue();
  sheet();
  await userEvent.click(await screen.findByRole("button", { name: "Supprimer a@gmail.com" }));
  expect(remove).toHaveBeenCalledWith(1);
});

it("« Ouvrir la console » ouvre la page du guide ; le formulaire Google envoie identifiant et secret", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([]);
  const openGuide = vi.spyOn(accountsIpc, "openGuide").mockResolvedValue();
  const setApp = vi.spyOn(accountsIpc, "setGoogleApp").mockResolvedValue();
  sheet();
  const google = (await screen.findByText("Configurer Google (Gmail et Agenda)")).closest("details")!;
  await userEvent.click(within(google).getAllByRole("button", { name: "Ouvrir la console" })[0]);
  expect(openGuide).toHaveBeenCalledWith("https://console.cloud.google.com/projectcreate");
  await userEvent.type(within(google).getByLabelText("Identifiant client Google"), "mon-id");
  await userEvent.type(within(google).getByLabelText("Secret client Google"), "mon-secret");
  await userEvent.click(within(google).getByRole("button", { name: "Enregistrer" }));
  expect(setApp).toHaveBeenCalledWith("mon-id", "mon-secret");
  await waitFor(() => expect((within(google).getByLabelText("Secret client Google") as HTMLInputElement).value).toBe(""));
});

it("s'ouvre seule au premier lancement (une fois)", async () => {
  vi.spyOn(accountsIpc, "list").mockResolvedValue([]);
  vi.mocked(accountsIpc.introTake).mockResolvedValue(true);
  const auto = vi.fn();
  render(<AccountsSheet open={false} onClose={() => {}} onAutoOpen={auto} />);
  await waitFor(() => expect(auto).toHaveBeenCalledTimes(1));
  expect(accountsIpc.introTake).toHaveBeenCalledTimes(1);
});
