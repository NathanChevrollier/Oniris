import { invoke, isTauri } from "../../lib/ipc";

/** Miroir de `oniris_core::accounts::AccountView`. */
export type AccountView = { id: number; provider: "google" | "spotify"; label: string; color: string; status: "ok" | "reauth" };

/** Miroir de `accounts::PALETTE` (couleurs de tokens.css). */
export const PALETTE = ["#ff7448", "#0a84ff", "#30d158", "#ffd60a", "#bf5af2", "#64d2ff", "#ff3d7f", "#5e5ce6"];

/** Noms français des couleurs de la palette, dans le même ordre. */
export const COLOR_NAMES = ["orange", "bleu", "vert", "jaune", "violet", "cyan", "rose", "indigo"];

/** Message d'une erreur de commande (chaîne côté Rust). */
export const errorText = (e: unknown, fallback: string) => (typeof e === "string" && e ? e : fallback);

const fakeAccounts: AccountView[] = [
  { id: 1, provider: "google", label: "prenom.nom@gmail.com", color: PALETTE[0], status: "ok" },
  { id: 2, provider: "google", label: "projets@gmail.com", color: PALETTE[1], status: "reauth" },
  { id: 3, provider: "spotify", label: "Prénom", color: PALETTE[2], status: "ok" },
];

const done = () => Promise.resolve();

export const accountsIpc = {
  list: (): Promise<AccountView[]> => isTauri ? invoke("accounts_list") : Promise.resolve(fakeAccounts),
  /** Le secret part au coffre Windows ; il n'est jamais renvoyé. Identifiant vide : efface la configuration. */
  setGoogleApp: (clientId: string, clientSecret: string): Promise<void> =>
    isTauri ? invoke("accounts_set_google_app", { clientId, clientSecret }) : done(),
  setSpotifyApp: (clientId: string): Promise<void> => isTauri ? invoke("accounts_set_spotify_app", { clientId }) : done(),
  /** Ouvre le navigateur et attend l'autorisation (5 minutes au plus). */
  connect: (provider: AccountView["provider"]): Promise<AccountView> =>
    isTauri ? invoke("accounts_connect", { provider }) : Promise.resolve(fakeAccounts[0]),
  /** Annule la connexion en cours (libère le port). */
  cancel: (): Promise<void> => isTauri ? invoke("accounts_cancel") : done(),
  remove: (id: number): Promise<void> => isTauri ? invoke("accounts_remove", { id }) : done(),
  setColor: (id: number, color: string): Promise<void> => isTauri ? invoke("accounts_set_color", { id, color }) : done(),
  /** Vrai une seule fois, au premier lancement sans compte ni identifiant client. */
  introTake: (): Promise<boolean> => isTauri ? invoke("accounts_intro_take") : Promise.resolve(false),
  openGuide: (url: string): Promise<void> => isTauri ? invoke("accounts_open_guide", { url }) : done(),
};
