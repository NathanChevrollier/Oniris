import { render, screen, cleanup, within, waitFor, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { MailCard } from "./MailCard";
import { csp } from "./MailReader";
import { mailIpc, type MailRow } from "./api";
import { accountsIpc, PALETTE, type AccountView } from "../accounts/api";
import { ConfirmQueue } from "../../panel/ConfirmQueue";
import { usePanelStore } from "../../panel/store";
import type { PendingAction, ToolOutcome } from "../../lib/ipc";

afterEach(() => { cleanup(); vi.restoreAllMocks(); usePanelStore.setState({ pending: [] }); });
beforeEach(() => usePanelStore.setState({ open: true }));

const done: ToolOutcome = { Done: { action_id: 1, output: {} } };
const acc = (id: number, status: AccountView["status"] = "ok"): AccountView => ({ id, provider: "google", label: `c${id}@gmail.com`, color: PALETTE[id], status });
const mk = (id: string, bucket: MailRow["bucket"], unread: boolean, extra: Partial<MailRow> = {}): MailRow => ({
  account_id: 1, id, thread_id: id, from_name: `Exp ${id}`, from_addr: `${id}@x.fr`, subject: `Sujet ${id}`, snippet: `extrait ${id}`,
  date_ms: Date.now() - 60_000, unread, bucket, summary: null, tag: null, ...extra,
});
const setup = (mails: MailRow[], accounts: AccountView[] = [acc(1), acc(2)], onAccounts = vi.fn()) => {
  vi.spyOn(mailIpc, "list").mockResolvedValue(mails);
  vi.spyOn(accountsIpc, "list").mockResolvedValue(accounts);
  render(<MailCard onAccounts={onAccounts} />);
  return onAccounts;
};
const body = { html: "<p>Salut</p>", text: "Salut", attachments: [], links: ["https://exemple.fr/a", "https://autre.fr/b"], truncated: false };

it("onglets : filtrent et comptent les non-lus", async () => {
  setup([mk("1", "todo", true), mk("2", "todo", true), mk("3", "read", false), mk("4", "read", true), mk("5", "filtered", false)]);
  await screen.findByText("Sujet 1");
  expect(screen.getByRole("tab", { name: /À traiter/ }).textContent).toBe("À traiter2");
  expect(screen.getByRole("tab", { name: /À lire/ }).textContent).toBe("À lire1");
  expect(screen.getByRole("tab", { name: /Filtrés/ }).textContent).toBe("Filtrés0");
  expect(screen.queryByText("Sujet 3")).toBeNull();
  await userEvent.click(screen.getByRole("tab", { name: /À lire/ }));
  expect(screen.getByText("Sujet 3")).toBeTruthy();
  expect(screen.queryByText("Sujet 1")).toBeNull();
});

it("la pastille porte la couleur du compte ; texte non fiable rendu en texte", async () => {
  setup([mk("1", "todo", true, { account_id: 2, from_name: "<b>x</b>", tag: "Bug", summary: "Résumé IRIS" })]);
  const dot = await screen.findByTestId("pastille");
  expect(dot.style.getPropertyValue("--c")).toBe(PALETTE[2]);
  expect(screen.getByText("<b>x</b>")).toBeTruthy(); // pas interprété comme HTML
  expect(screen.getByText("Bug")).toBeTruthy();
  expect(screen.getByText("Résumé IRIS")).toBeTruthy();
});

it("Archiver appelle archive, Lu appelle setRead", async () => {
  setup([mk("1", "todo", true)]);
  const archive = vi.spyOn(mailIpc, "archive").mockResolvedValue(done);
  const read = vi.spyOn(mailIpc, "setRead").mockResolvedValue(done);
  const row = (await screen.findByText("Sujet 1")).closest("li")!;
  await userEvent.click(within(row).getByRole("button", { name: "Archiver" }));
  expect(archive).toHaveBeenCalledWith(1, "1");
  await userEvent.click(within(row).getByRole("button", { name: "Lu" }));
  expect(read).toHaveBeenCalledWith(1, "1", true);
});

it("lecteur : iframe sans scripts, CSP sans https avant « Afficher les images », puis https pour ce mail seulement", async () => {
  setup([mk("1", "todo", true), mk("2", "todo", false)]);
  vi.spyOn(mailIpc, "read").mockResolvedValue(body);
  await userEvent.click(await screen.findByText("Sujet 1"));
  const frame = await screen.findByTitle("Message") as HTMLIFrameElement;
  const sb = frame.getAttribute("sandbox")!;
  expect(sb).toBe("allow-popups-to-escape-sandbox");
  expect(sb).not.toMatch(/allow-scripts|allow-same-origin/);
  expect(frame.getAttribute("srcdoc")).toContain(`<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data: cid:">`);
  expect(csp(true)).toBe("default-src 'none'; style-src 'unsafe-inline'; img-src data: cid: https:");
  await userEvent.click(screen.getByRole("button", { name: /Afficher les images/ }));
  expect(frame.getAttribute("srcdoc")).toContain("img-src data: cid: https:");
  // Autre mail : retour à la CSP stricte.
  await userEvent.click(screen.getByRole("button", { name: /Retour/ }));
  await userEvent.click(await screen.findByText("Sujet 2"));
  const f2 = await screen.findByTitle("Message");
  expect(f2.getAttribute("srcdoc")).not.toContain("https:");
  expect(screen.getByRole("button", { name: /Répondre avec IRIS/ })).toBeEnabled();
});

it("message tronqué : mention et bouton « Ouvrir dans Gmail » ; rien si complet", async () => {
  setup([mk("1", "todo", true)]);
  const read = vi.spyOn(mailIpc, "read").mockResolvedValue({ ...body, truncated: true });
  const open = vi.spyOn(mailIpc, "openWeb").mockResolvedValue(done);
  await userEvent.click(await screen.findByText("Sujet 1"));
  expect(await screen.findByText(/Message tronqué/)).toBeTruthy();
  await userEvent.click(screen.getByText("Ouvrir dans Gmail"));
  expect(open).toHaveBeenCalledWith(1, "1");
  cleanup();
  setup([mk("1", "todo", true)]);
  read.mockResolvedValue(body);
  await userEvent.click(await screen.findByText("Sujet 1"));
  await screen.findByTitle("Message");
  expect(screen.queryByText(/Message tronqué/)).toBeNull();
});

it("liens listés avec leur URL, ouverts par open_link", async () => {
  setup([mk("1", "todo", true)]);
  vi.spyOn(mailIpc, "read").mockResolvedValue(body);
  const open = vi.spyOn(mailIpc, "openLink").mockResolvedValue(done);
  await userEvent.click(await screen.findByText("Sujet 1"));
  expect(await screen.findByText("Liens du message")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "https://autre.fr/b" }));
  expect(open).toHaveBeenCalledWith("https://autre.fr/b");
});

it("Composer : « Envoyer » appelle send avec les champs exacts ; le brouillon survit à la fermeture du lecteur", async () => {
  setup([mk("1", "todo", true, { subject: "Devis", from_addr: "marc@x.fr" })]);
  vi.spyOn(mailIpc, "read").mockResolvedValue(body);
  const send = vi.spyOn(mailIpc, "send").mockResolvedValue(done);
  await userEvent.click(await screen.findByText("Devis"));
  await userEvent.click(await screen.findByRole("button", { name: /^Répondre$/ }));
  expect((screen.getByLabelText("À") as HTMLInputElement).value).toBe("marc@x.fr");
  expect((screen.getByLabelText("Sujet") as HTMLInputElement).value).toBe("Re: Devis");
  await userEvent.type(screen.getByPlaceholderText("Ta réponse…"), "Ok\nmerci");
  await userEvent.click(screen.getByRole("button", { name: /Retour/ }));
  await userEvent.click(await screen.findByText("Devis"));
  expect((screen.getByPlaceholderText("Ta réponse…") as HTMLTextAreaElement).value).toBe("Ok\nmerci");
  await userEvent.click(screen.getByRole("button", { name: /Envoyer/ }));
  expect(send).toHaveBeenCalledWith(1, "marc@x.fr", "Re: Devis", "Ok\nmerci", "1");
});

it("Répondre avec IRIS : le brouillon remplit le composeur, la mention est visible, Claude indisponible propose le local", async () => {
  setup([mk("1", "todo", true, { subject: "Devis", from_addr: "marc@x.fr" })]);
  vi.spyOn(mailIpc, "read").mockResolvedValue(body);
  const draft = vi.spyOn(mailIpc, "draftReply").mockResolvedValueOnce("Bonjour Marc, c'est bon.");
  await userEvent.click(await screen.findByText("Devis"));
  expect(await screen.findByText("Le contenu de ce mail sera envoyé à Anthropic via Claude Code.")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: /Répondre avec IRIS/ }));
  expect(draft).toHaveBeenCalledWith(1, "1");
  await waitFor(() => expect((screen.getByPlaceholderText("Ta réponse…") as HTMLTextAreaElement).value).toBe("Bonjour Marc, c'est bon."));
  expect((screen.getByLabelText("Sujet") as HTMLInputElement).value).toBe("Re: Devis");

  draft.mockRejectedValueOnce(new Error("claude_unavailable"));
  const local = vi.spyOn(mailIpc, "draftReplyLocal").mockResolvedValue("Version locale.");
  await userEvent.click(screen.getByRole("button", { name: /Répondre avec IRIS/ }));
  await userEvent.click(await screen.findByRole("button", { name: "Version simple en local" }));
  expect(local).toHaveBeenCalledWith(1, "1");
  await waitFor(() => expect((screen.getByPlaceholderText("Ta réponse…") as HTMLTextAreaElement).value).toBe("Version locale."));
  expect(screen.queryByRole("button", { name: "Version simple en local" })).toBeNull();
});

it("sans compte : invite et ouverture de la feuille Comptes", async () => {
  const onAccounts = setup([], []);
  await userEvent.click(await screen.findByRole("button", { name: "Connecter un compte" }));
  expect(screen.getByText("Connecte un compte Gmail")).toBeTruthy();
  expect(onAccounts).toHaveBeenCalled();
});

it("compte à reconnecter : bandeau", async () => {
  setup([mk("1", "todo", true)], [acc(1), acc(2, "reauth")]);
  expect((await screen.findByText(/Reconnecte c2@gmail.com/))).toBeTruthy();
});

it("hors ligne : mention avec l'âge de la dernière relève, sans âge si inconnu", async () => {
  vi.spyOn(mailIpc, "status").mockResolvedValue({ last_ok_ms: Date.now() - 4 * 60_000 });
  setup([mk("1", "todo", true)]);
  await screen.findByText("Sujet 1");
  act(() => { window.dispatchEvent(new Event("offline")); });
  expect(await screen.findByText(/hors ligne · il y a 4 min/)).toBeTruthy();
  expect(screen.getByText("Sujet 1")).toBeTruthy();
});

it("hors ligne sans relève connue : « hors ligne » seul", async () => {
  vi.spyOn(mailIpc, "status").mockResolvedValue({ last_ok_ms: null });
  setup([mk("1", "todo", true)]);
  await screen.findByText("Sujet 1");
  act(() => { window.dispatchEvent(new Event("offline")); });
  const aside = await screen.findByText(/hors ligne/);
  expect(aside.textContent?.trim()).toBe("hors ligne");
});

it("HTML hostile : la CSP précède le contenu du mail", async () => {
  setup([mk("1", "todo", true)]);
  const html = `</head><meta http-equiv="refresh" content="0;url=https://x"><base href="https://x/">`;
  vi.spyOn(mailIpc, "read").mockResolvedValue({ ...body, html });
  await userEvent.click(await screen.findByText("Sujet 1"));
  const src = (await screen.findByTitle("Message")).getAttribute("srcdoc")!;
  expect(src.startsWith(`<!doctype html><meta http-equiv="Content-Security-Policy"`)).toBe(true);
  expect(src.indexOf("Content-Security-Policy")).toBeLessThan(src.indexOf(html));
});

it("erreur de validation hors envoi : garde son propre message", async () => {
  setup([mk("1", "todo", true)]);
  vi.spyOn(mailIpc, "archive").mockResolvedValue({ Failed: { action_id: 1, error: "arguments invalides : texte trop long" } });
  await userEvent.click(within((await screen.findByText("Sujet 1")).closest("li")!).getByRole("button", { name: "Archiver" }));
  expect((await screen.findByRole("alert")).textContent).toBe("Ce texte est trop long.");
});

it("confirmation d'envoi : texte complet, espaces conservés, défilement", () => {
  const long = Array.from({ length: 300 }, (_, i) => `ligne ${i}   avec  espaces`).join("\n");
  const details = `Compte : a\nÀ : b\nSujet : Re: c\n\n${long}`;
  const p: PendingAction = { action_id: 9, tool: "mail.send", text: { summary: "Envoyer", details, risk: "High" }, provenance: "User", expires_at_ms: 0 };
  usePanelStore.setState({ pending: [p] });
  render(<ConfirmQueue subscribe={() => Promise.resolve(() => {})} resolve={() => Promise.resolve(done)} />);
  const pre = document.querySelector("pre")!;
  expect(pre.textContent).toBe(details);
  expect(pre.textContent).not.toMatch(/…|\.\.\./);
  expect(pre.style.whiteSpace).toBe("pre-wrap");
  expect(pre.style.overflowY).toBe("auto");
  expect(pre.style.maxHeight).not.toBe("");
});

it("approbation échouée (arguments masqués après redémarrage) : « Action expirée, recommence. »", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const p: PendingAction = { action_id: 9, tool: "mail.send", text: { summary: "Envoyer", details: "d", risk: "High" }, provenance: "User", expires_at_ms: 0 };
  usePanelStore.setState({ pending: [p] });
  const resolve = vi.fn().mockResolvedValue({ Failed: { action_id: 9, error: "arguments invalides : invalid type: map, expected a string" } });
  render(<ConfirmQueue subscribe={() => Promise.resolve(() => {})} resolve={resolve} />);
  act(() => { vi.advanceTimersByTime(600); });
  vi.useRealTimers();
  fireEvent.click(screen.getByRole("button", { name: "Autoriser" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Action expirée, recommence."));
});

it("approbation échouée d'un autre outil : pas de « expirée »", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const p: PendingAction = { action_id: 8, tool: "mail.delete", text: { summary: "Supprimer", details: "d", risk: "Medium" }, provenance: "User", expires_at_ms: 0 };
  usePanelStore.setState({ pending: [p] });
  const resolve = vi.fn().mockResolvedValue({ Failed: { action_id: 8, error: "arguments invalides : mail introuvable" } });
  render(<ConfirmQueue subscribe={() => Promise.resolve(() => {})} resolve={resolve} />);
  act(() => { vi.advanceTimersByTime(600); });
  vi.useRealTimers();
  fireEvent.click(screen.getByRole("button", { name: "Autoriser" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("L'action a échoué."));
});
