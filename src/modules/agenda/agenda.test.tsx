import { render, screen, cleanup, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { MonthGrid } from "./MonthGrid";
import { DayList } from "./DayList";
import { AgendaCard } from "./AgendaCard";
import { agendaIpc, calendarsIpc } from "./api";
import { CalendarsManager } from "./Calendars";
import { usePanelStore } from "../../panel/store";
import { Timeline } from "../../panel/Timeline";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
beforeEach(() => usePanelStore.setState({ open: false }));

it("MonthGrid : le 1er novembre 2026 (dimanche) est en 7e colonne, jour courant et points marqués", () => {
  const { container } = render(<MonthGrid year={2026} month={11} today={4} selected={null} withEvents={[4, 12]} onSelect={() => {}} />);
  const cells = [...container.querySelectorAll(".cal > span, .cal > button")];
  expect(cells.findIndex((c) => c.textContent === "1")).toBe(6); // 6 cases vides avant le dimanche
  expect(container.querySelectorAll(".cal > .d")).toHaveLength(7);
  expect(screen.getByRole("button", { name: /^4 novembre 2026/ })).toHaveClass("today", "has");
  expect(screen.getByRole("button", { name: /^12 novembre 2026/ })).toHaveClass("has");
  expect(screen.getByRole("button", { name: /^5 novembre 2026/ })).not.toHaveClass("today", "has");
  expect(container.querySelectorAll(".cal button")).toHaveLength(30);
});

it("MonthGrid : accessibilité (libellés de jour, aujourd'hui, en-têtes)", () => {
  const { container } = render(<MonthGrid year={2026} month={11} today={4} selected={null} withEvents={[4]} onSelect={() => {}} />);
  expect(screen.getByRole("button", { name: "4 novembre 2026, avec des événements" })).toHaveAttribute("aria-current", "date");
  expect(screen.getByRole("button", { name: "5 novembre 2026" })).not.toHaveAttribute("aria-current");
  expect(container.querySelector('abbr[title="lundi"]')).toHaveTextContent("L");
  expect(container.querySelector('abbr[title="dimanche"]')).toHaveTextContent("D");
});

it("MonthGrid : un clic sélectionne le jour", async () => {
  const onSelect = vi.fn();
  render(<MonthGrid year={2026} month={11} today={null} selected={3} withEvents={[]} onSelect={onSelect} />);
  await userEvent.click(screen.getByRole("button", { name: /^9 novembre 2026/ }));
  expect(onSelect).toHaveBeenCalledWith(9);
});

it("DayList vide : « Rien de prévu, journée libre. »", () => {
  render(<DayList events={[]} onDelete={() => {}} />);
  expect(screen.getByText("Rien de prévu, journée libre.")).toBeInTheDocument();
});

function mockRead() {
  vi.spyOn(agendaIpc, "monthDays").mockResolvedValue([]);
  return vi.spyOn(agendaIpc, "between").mockResolvedValue([]);
}

it("AgendaCard : une date introuvable affiche l'erreur en français", async () => {
  usePanelStore.setState({ open: true });
  mockRead();
  vi.spyOn(agendaIpc, "create").mockResolvedValue({ Failed: { action_id: 1, error: "arguments invalides : date introuvable" } });
  render(<AgendaCard />);
  await userEvent.type(screen.getByRole("textbox", { name: "Ajouter un événement" }), "dentiste{Enter}");
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Date introuvable — essaie « demain 14h dentiste »"));
});

it("AgendaCard : une commande en erreur affiche un message inline", async () => {
  usePanelStore.setState({ open: true });
  mockRead();
  vi.spyOn(agendaIpc, "create").mockRejectedValue(new Error("boom"));
  render(<AgendaCard />);
  await userEvent.type(screen.getByRole("textbox", { name: "Ajouter un événement" }), "demain 10h x{Enter}");
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Agenda indisponible"));
});

it("AgendaCard : le jour affiché suit minuit tant qu'aucun jour n'est choisi", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.setSystemTime(new Date(2026, 10, 4, 23, 59, 30));
  usePanelStore.setState({ open: true });
  const between = mockRead();
  render(<AgendaCard />);
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByRole("button", { name: /^4 novembre/ })).toHaveAttribute("aria-current", "date");
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(screen.getByRole("button", { name: /^5 novembre/ })).toHaveAttribute("aria-current", "date");
  expect(between.mock.calls.length).toBeGreaterThanOrEqual(2);
});

it("événement Google : sans bouton de suppression dans la liste, bloc coloré dans la frise", async () => {
  const base = { end_ms: null, all_day: false, notes: null };
  const local = { ...base, id: 3, key: "l:3", source: "local" as const, readonly: false, title: "Dentiste", start_ms: 1, color: null };
  const google = { ...base, id: 0, key: "g:1:c:e", source: "google" as const, readonly: true, title: "Réunion", start_ms: 2, color: "#0a84ff" };
  const onDelete = vi.fn();
  render(<DayList events={[local, google]} onDelete={onDelete} />);
  expect(screen.getByRole("button", { name: "Supprimer Dentiste" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Supprimer Réunion" })).toBeNull();
  cleanup();
  const at = (h: number) => new Date(2026, 10, 5, h);
  const { container } = render(<Timeline startHour={8} endHour={20} items={[{ start: at(10), end: at(11), label: "Réunion", kind: "event", color: "#0a84ff", readonly: true }]} />);
  const ev = container.querySelector(".ev") as HTMLElement;
  expect(ev).toHaveTextContent("Réunion");
  expect(ev.style.getPropertyValue("--c")).toBe("#0a84ff");
  expect(ev).toHaveAttribute("data-readonly", "true");
  expect(ev.querySelector("button")).toBeNull();
});

it("DayList : doublons fusionnés, une pastille par calendrier d'origine et le nom du calendrier", () => {
  const sources = [1, 2, 3].map((i) => ({ calendar: "Jours fériés", account: `c${i}@gmail.com`, color: `#00000${i}` }));
  const e = { id: 0, key: "g:1:h:x", source: "google" as const, readonly: true, title: "Armistice", start_ms: 1, end_ms: 2, all_day: true, color: "#000001", notes: null, calendar: "Jours fériés", sources };
  const { container } = render(<DayList events={[e]} onDelete={() => {}} />);
  expect(screen.getAllByText("Armistice")).toHaveLength(1);
  expect(container.querySelectorAll(".dl-src .dot")).toHaveLength(3);
  expect(screen.getByRole("img", { name: /Dans 3 calendriers : Jours fériés \(c1@gmail.com\)/ })).toBeInTheDocument();
  expect(screen.getByText("Jours fériés")).toBeInTheDocument();
});

it("DayList : état de chargement distinct de la journée libre", () => {
  render(<DayList events={[]} onDelete={() => {}} loading />);
  expect(screen.getByRole("status")).toHaveTextContent("Chargement");
  expect(screen.queryByText("Rien de prévu, journée libre.")).toBeNull();
});

it("AgendaCard : « … » ouvre les calendriers affichés ; décocher masque, Échap ferme", async () => {
  usePanelStore.setState({ open: true });
  mockRead();
  const cal = (id: number, name: string, extra = {}) => ({ id, kind: "google" as const, account_id: 1, account: "moi@gmail.com", name, color: "#0a84ff", primary: false, visible: true, duplicate: false, url: null, fetched_ms: null, error: null, events: 0, ...extra });
  vi.spyOn(calendarsIpc, "list").mockResolvedValue([cal(1, "Perso", { primary: true }), cal(2, "Jours fériés", { visible: false, duplicate: true }),
    { ...cal(3, "Club"), kind: "file", account_id: null, account: null, events: 12 }]);
  const setVisible = vi.spyOn(calendarsIpc, "setVisible").mockResolvedValue();
  const remove = vi.spyOn(calendarsIpc, "remove").mockResolvedValue();
  vi.spyOn(calendarsIpc, "refresh").mockResolvedValue();
  render(<AgendaCard />);
  await userEvent.click(screen.getByRole("button", { name: "Calendriers affichés" }));
  const pop = await screen.findByRole("dialog", { name: "Calendriers affichés" });
  expect(await screen.findByText("Déjà affiché depuis un autre compte")).toBeInTheDocument();
  expect(screen.getByText("12 événements")).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: /Jours fériés/ })).not.toBeChecked();
  expect(screen.queryByRole("button", { name: "Supprimer le calendrier Perso" })).toBeNull();
  await userEvent.click(screen.getByRole("checkbox", { name: /Perso/ }));
  expect(setVisible).toHaveBeenCalledWith(1, false);
  await userEvent.click(screen.getByRole("button", { name: "Supprimer le calendrier Club" }));
  expect(remove).toHaveBeenCalledWith(3);
  expect(pop).toHaveTextContent("Importer un fichier .ics");
  await userEvent.keyboard("{Escape}");
  expect(screen.queryByRole("dialog", { name: "Calendriers affichés" })).toBeNull();
});

it("abonnement : une erreur de Rust s'affiche en ligne, un succès donne le compte rendu", async () => {
  usePanelStore.setState({ open: true });
  vi.spyOn(calendarsIpc, "list").mockResolvedValue([]);
  const sub = vi.spyOn(calendarsIpc, "subscribe").mockRejectedValueOnce("adresse invalide : colle un lien https:// ou webcal:// vers un calendrier .ics")
    .mockResolvedValueOnce({ name: "École", events: 42, truncated: false });
  render(<CalendarsManager />);
  const input = screen.getByRole("textbox", { name: "Adresse d'un calendrier en ligne" });
  await userEvent.type(input, "http://x{Enter}");
  expect(await screen.findByRole("alert")).toHaveTextContent("adresse invalide");
  await userEvent.clear(input);
  await userEvent.type(input, "webcal://ecole.fr/a.ics{Enter}");
  expect(await screen.findByText("École : 42 événements")).toBeInTheDocument();
  expect(sub).toHaveBeenLastCalledWith("webcal://ecole.fr/a.ics");
  expect(input).toHaveValue("");
});
