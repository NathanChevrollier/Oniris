import { render, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { SitesSection, addError } from "./SitesSection";
import { sitesIpc, counter, type SiteView } from "./api";
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
