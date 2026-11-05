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
const body = { html: "<p>Salut</p>", text: "Salut", attachments: [], links: ["https://exemple.fr/a", "https://autre.fr/b"] };

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
  expect(screen.getByRole("button", { name: /Répondre avec IRIS/ })).toBeDisabled();
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

it("hors ligne : mention avec le cache", async () => {
  setup([mk("1", "todo", true)]);
  await screen.findByText("Sujet 1");
  act(() => { window.dispatchEvent(new Event("offline")); });
  expect(screen.getByText(/hors ligne · il y a 1 min/)).toBeTruthy();
  expect(screen.getByText("Sujet 1")).toBeTruthy();
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
