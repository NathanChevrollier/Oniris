import { invoke, isTauri, type ToolOutcome } from "../../lib/ipc";

export type Bucket = "todo" | "read" | "filtered";

/** Miroir de `oniris_core::mail::tools::MailRow`. Texte non fiable : toujours rendu comme texte. */
export type MailRow = {
  account_id: number; id: string; thread_id: string; from_name: string; from_addr: string; subject: string;
  snippet: string; date_ms: number; unread: boolean; bucket: Bucket; summary: string | null; tag: string | null;
};

export type Attachment = { filename: string; mime_type: string; size: number; attachment_id: string | null };
/** Sortie de `mail.read` : HTML déjà assaini, liens extraits (jamais cliquables dans l'iframe). */
export type MailBody = { html: string; text: string; attachments: Attachment[]; links: string[]; truncated: boolean };

const MIN = 60_000;
const m = (id: string, account_id: number, from_name: string, from_addr: string, subject: string, snippet: string, ago: number,
  bucket: Bucket, unread: boolean, summary: string | null = null, tag: string | null = null): MailRow =>
  ({ account_id, id, thread_id: id, from_name, from_addr, subject, snippet, date_ms: Date.now() - ago * MIN, unread, bucket, summary, tag });

/** Mails factices pour l'aperçu navigateur (hors Tauri). */
const fake = (): MailRow[] => [
  m("a", 1, "Marc Dupont", "marc@dupont.example", "Re: Devis refonte site vitrine", "Parfait pour moi, peux-tu confirmer la date ?", 25, "todo", true,
    "Valide le devis, attend une date de livraison.", "Aujourd'hui"),
  m("b", 1, "OVHcloud", "billing@ovh.example", "Votre facture FR-2026-11-0042 est disponible", "Montant : 14,99 €", 90, "todo", false, "14,99 € · prélèvement le 8 nov. Rien à faire."),
  m("c", 2, "GitHub", "noreply@github.example", "[zenytt] Issue #41 : crash au démarrage sans Docker", "Le démarrage échoue lorsque Docker est absent.", 1500, "todo", false,
    "Probablement lié au check Docker hôte.", "Bug"),
  m("d", 2, "Camille Roux", "camille@example.org", "Photos du week-end", "Je t'envoie le dossier partagé.", 300, "read", true),
  m("e", 1, "Léa Martin", "lea@example.org", "Dîner samedi ?", "On se retrouve vers 20 h ?", 700, "read", false),
  m("f", 1, "Boutique Soleil", "promo@soleil.example", "-30 % ce week-end seulement", "Profitez de nos offres exclusives.", 200, "filtered", true),
];

const fakeBody: MailBody = {
  html: "<p>Bonjour,</p><p>Le devis me convient. Peux-tu confirmer la date de livraison ?</p><p>Marc</p>",
  text: "Bonjour,\n\nLe devis me convient. Peux-tu confirmer la date de livraison ?\n\nMarc",
  attachments: [{ filename: "devis.pdf", mime_type: "application/pdf", size: 84_000, attachment_id: "x" }],
  links: ["https://dupont.example/devis/2026-11"],
  truncated: false,
};

const ok = (): Promise<ToolOutcome> => Promise.resolve({ Done: { action_id: 0, output: {} } });

/** Plus ancienne relève réussie des comptes ok (`null` : inconnue). */
export type MailStatus = { last_ok_ms: number | null };

export const mailIpc = {
  status: (): Promise<MailStatus> => isTauri ? invoke("mail_status") : Promise.resolve({ last_ok_ms: Date.now() - 4 * MIN }),
  list: (): Promise<MailRow[]> => isTauri ? invoke("mail_list", {}) : Promise.resolve(fake()),
  /** Lit un mail (le marque lu). Rejette si l'outil échoue. */
  read: async (account: number, id: string): Promise<MailBody> => {
    if (!isTauri) return fakeBody;
    const out = await invoke<ToolOutcome>("mail_read", { account, id });
    if ("Done" in out) return out.Done.output as MailBody;
    throw new Error("Failed" in out ? out.Failed.error : "mail indisponible");
  },
  setRead: (account: number, id: string, read: boolean): Promise<ToolOutcome> => isTauri ? invoke("mail_set_read", { account, id, read }) : ok(),
  archive: (account: number, id: string): Promise<ToolOutcome> => isTauri ? invoke("mail_archive", { account, id }) : ok(),
  delete: (account: number, id: string): Promise<ToolOutcome> => isTauri ? invoke("mail_delete", { account, id }) : ok(),
  /** Déclenche la carte de confirmation du `ConfirmQueue`. */
  send: (account: number, to: string, subject: string, body: string, replyToId: string | null): Promise<ToolOutcome> =>
    isTauri ? invoke("mail_send", { account, to, subject, body, replyToId }) : ok(),
  openWeb: (account: number, id: string): Promise<ToolOutcome> => isTauri ? invoke("mail_open_web", { account, id }) : ok(),
  openLink: (url: string): Promise<ToolOutcome> => isTauri ? invoke("mail_open_link", { url }) : ok(),
};
