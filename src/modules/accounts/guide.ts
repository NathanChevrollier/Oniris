/** Redirection en boucle locale, déclarée telle quelle chez Spotify (miroir de `oauth::REDIRECT_URI`). */
export const REDIRECT_URI = "http://127.0.0.1:47829/callback";

/** Scopes Google demandés (miroir de `oauth::providers::GOOGLE_SCOPES`). */
export const GOOGLE_SCOPES = [
  "openid",
  "email",
  "https://www.googleapis.com/auth/gmail.modify",
  "https://www.googleapis.com/auth/calendar.readonly",
];

export type Step = {
  text: string;
  /** Bouton « Ouvrir la console » : page des guides seulement (liste blanche côté Rust). */
  open?: { label: string; url: string };
  /** Valeur à copier telle quelle. */
  copy?: { label: string; value: string };
  /** Formulaire d'identifiants à la fin du guide. */
  form?: "google" | "spotify";
};

export type Guide = { id: "google" | "spotify" | "orange"; title: string; steps: Step[]; note?: string };

export const GUIDES: Guide[] = [
  {
    id: "google",
    title: "Configurer Google (Gmail et Agenda)",
    steps: [
      { text: "Crée un projet sur console.cloud.google.com.", open: { label: "Ouvrir la console", url: "https://console.cloud.google.com/projectcreate" } },
      { text: "Active Gmail API et Google Calendar API.", open: { label: "Ouvrir la console", url: "https://console.cloud.google.com/apis/library" } },
      {
        text: "Écran de consentement : choisis « Externe », puis ajoute les 4 scopes.",
        open: { label: "Ouvrir la console", url: "https://console.cloud.google.com/apis/credentials/consent" },
        copy: { label: "Copier les scopes", value: GOOGLE_SCOPES.join("\n") },
      },
      { text: "Clique sur « Publier l'application » : le statut doit passer à « En production »." },
      { text: "Crée un identifiant de type « Application de bureau ».", open: { label: "Ouvrir la console", url: "https://console.cloud.google.com/apis/credentials" } },
      { text: "Colle l'identifiant client et le secret client.", form: "google" },
    ],
    note: "À la première connexion, Google affiche « Google n'a pas validé cette application » : clique sur « Paramètres avancés », puis sur « Accéder à… ».",
  },
  {
    id: "spotify",
    title: "Configurer Spotify",
    steps: [
      { text: "Ouvre developer.spotify.com/dashboard et crée une app.", open: { label: "Ouvrir la console", url: "https://developer.spotify.com/dashboard" } },
      { text: "Pour l'URI de redirection, saisis exactement :", copy: { label: "Copier l'URI", value: REDIRECT_URI } },
      { text: "Coche « Web API »." },
      { text: "Colle le Client ID.", form: "spotify" },
    ],
    note: "Le compte propriétaire de l'app doit être Premium.",
  },
  {
    id: "orange",
    title: "Ajouter une adresse Orange (via Gmail)",
    steps: [
      { text: "Dans le webmail Orange, ouvre Préférences, puis Transfert, et saisis ton adresse Gmail." },
      {
        text: "Dans Gmail, crée un filtre sur l'adresse Orange pour y appliquer une étiquette.",
        open: { label: "Ouvrir Gmail", url: "https://mail.google.com/mail/u/0/#settings/filters" },
      },
    ],
    note: "Aucun code dans Oniris : les mails Orange arrivent par ton compte Gmail.",
  },
];
