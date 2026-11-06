import { render, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { SitesSection, addError } from "./SitesSection";
import { sitesIpc, vpsIpc, counter, type SiteView, type VpsStatus } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => { usePanelStore.setState({ open: true }); });

const v = (name: string, state: SiteView["state"], last_ms: number | null = 100): SiteView => ({ name, url: `https://${name.toLowerCase()}.example.com`, state, last_ms, spark: [90, 100, 110] });
const mix = [v("Alpha", "up"), v("Beta", "slow", 2400), v("Gamma", "down", null), v("Delta", "up"), v("Nouveau", "unknown", null)];

it("compte les sites en ligne (lent inclus, non sondé exclu)", () => {
  expect(counter(mix)).toEqual({ online: 3, total: 4 });
});

it("affiche « N/M en ligne » et une pastille par état", async () => {
  vi.spyOn(sitesIpc, "list").mockResolvedValue(mix);
  render(<SitesSection />);
  expect(await screen.findByText("3/4 en ligne")).toBeInTheDocument();
  const classOf = (label: string) => screen.getAllByRole("img", { name: label })[0].className;
  expect(classOf("en ligne")).toContain("up");
  expect(classOf("lent")).toContain("slow");
  expect(classOf("en panne")).toContain("down");
  expect(screen.getByText("2,4 s".replace(",", "."))).toBeInTheDocument();
});

it("tous les sites hors ligne : « Hors ligne »", async () => {
  vi.spyOn(sitesIpc, "list").mockResolvedValue([v("A", "offline"), v("B", "offline")]);
  render(<SitesSection />);
  expect(await screen.findByText("Hors ligne")).toBeInTheDocument();
});

it("ajoute un site (nom facultatif) puis recharge", async () => {
  const list = vi.spyOn(sitesIpc, "list").mockResolvedValue([]);
  const add = vi.spyOn(sitesIpc, "add").mockResolvedValue();
  render(<SitesSection />);
  await userEvent.click(await screen.findByRole("button", { name: "Modifier les sites" }));
  await userEvent.type(screen.getByLabelText("Adresse du site à surveiller"), "https://blog.example.org");
  await userEvent.click(screen.getByRole("button", { name: /Surveiller/ }));
  expect(add).toHaveBeenCalledWith("https://blog.example.org", "");
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
});

it("retire un site et montre l'erreur d'ajout", async () => {
  vi.spyOn(sitesIpc, "list").mockResolvedValue([v("Alpha", "up")]);
  const remove = vi.spyOn(sitesIpc, "remove").mockResolvedValue();
  vi.spyOn(sitesIpc, "add").mockRejectedValue("échec : sites : adresse http(s) de 300 caractères au plus");
  render(<SitesSection />);
  await userEvent.click(await screen.findByRole("button", { name: "Modifier les sites" }));
  await userEvent.type(screen.getByLabelText("Adresse du site à surveiller"), "ftp://x");
  await userEvent.click(screen.getByRole("button", { name: /Surveiller/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Adresse invalide");
  await userEvent.click(screen.getByRole("button", { name: "Retirer Alpha" }));
  expect(remove).toHaveBeenCalledWith("https://alpha.example.com");
  expect(addError("sites : adresse déjà surveillée")).toBe("Ce site est déjà surveillé.");
});

it("affiche les problèmes d'abord, sans changer l'ordre enregistré", async () => {
  const stored = [v("Alpha", "up"), v("Beta", "slow", 2400), v("Gamma", "down", null), v("Delta", "unknown", null)];
  vi.spyOn(sitesIpc, "list").mockResolvedValue(stored);
  render(<SitesSection />);
  await screen.findByText("Gamma");
  expect(screen.getAllByRole("listitem").map((li) => li.querySelector(".site-name")?.textContent)).toEqual(["Gamma", "Beta", "Delta", "Alpha"]);
  expect(stored.map((s) => s.name)).toEqual(["Alpha", "Beta", "Gamma", "Delta"]);
});

const vps = (over: Partial<VpsStatus> = {}): VpsStatus => ({ zenytt: true, view: { server: "vps-ovh", cpu_pct: 23.4, mem_pct: 61.2, disk_pct: 48.7, load1: 0.42, alerts: 0, at_ms: 1 }, error: null, ...over });

it("ligne VPS : métriques arrondies, masquée sans Zenytt", async () => {
  vi.spyOn(sitesIpc, "list").mockResolvedValue([]);
  const status = vi.spyOn(vpsIpc, "status").mockResolvedValue(vps());
  const { unmount } = render(<SitesSection />);
  expect(await screen.findByText("vps-ovh")).toBeInTheDocument();
  expect(screen.getByText("23 %")).toBeInTheDocument();
  expect(screen.getByText("0.42")).toBeInTheDocument();
  expect(screen.getByRole("img", { name: "VPS en ligne" })).toBeInTheDocument();
  unmount();
  status.mockResolvedValue({ zenytt: false, view: null, error: null });
  render(<SitesSection />);
  await screen.findByText("Aucun site surveillé");
  expect(screen.queryByText(/CPU|VPS/)).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Modifier les sites" }));
  expect(screen.queryByRole("button", { name: /Importer depuis Zenytt/ })).toBeNull();
});

it("VPS indisponible : raison courte, dernier relevé estompé", async () => {
  vi.spyOn(sitesIpc, "list").mockResolvedValue([]);
  vi.spyOn(vpsIpc, "status").mockResolvedValue(vps({ view: null, error: "clé du serveur jamais approuvée" }));
  render(<SitesSection />);
  expect(await screen.findByText("Indisponible : clé du serveur jamais approuvée")).toBeInTheDocument();
  expect(screen.getByRole("img", { name: "VPS indisponible" })).toBeInTheDocument();
});

it("importe les sites depuis Zenytt puis recharge", async () => {
  const list = vi.spyOn(sitesIpc, "list").mockResolvedValue([]);
  vi.spyOn(vpsIpc, "status").mockResolvedValue(vps());
  const imp = vi.spyOn(vpsIpc, "importSites").mockResolvedValue(2);
  render(<SitesSection />);
  await userEvent.click(await screen.findByRole("button", { name: "Modifier les sites" }));
  await userEvent.click(await screen.findByRole("button", { name: /Importer depuis Zenytt/ }));
  expect(imp).toHaveBeenCalled();
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
  imp.mockResolvedValue(0);
  await userEvent.click(screen.getByRole("button", { name: /Importer depuis Zenytt/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Aucun nouveau site HTTPS");
});
