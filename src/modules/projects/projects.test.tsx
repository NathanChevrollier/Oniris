import { render, screen, cleanup, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { ProjectsCard } from "./ProjectsCard";
import { projectsIpc, ago, type Project } from "./api";
import { usePanelStore } from "../../panel/store";
import * as focus from "../../lib/focusCard";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const withRoots = (roots: string[]) => vi.spyOn(projectsIpc, "roots").mockImplementation((cb) => { cb(roots); return () => {}; });
beforeEach(() => { usePanelStore.setState({ open: true }); withRoots(["/R"]); });

const DAY = 86_400_000;
const proj = (name: string, extra: Partial<Project> = {}): Project => ({
  path: `/P/${name}`, name, last_activity_ms: Date.now() - 3 * DAY, branch: "main", dirty: 0, activity_14d: Array(14).fill(0),
  plan: null, last_session: null, pinned: false, hidden: false, ...extra,
});
const plan = { file: "PLAN.md", done: 7, total: 12, current: "l'heuristique octile" };
const four = () => [proj("Alpha", { plan }), proj("Beta", { last_session: { id: "s", updated_ms: 1, title: "signature des builds" } }), proj("Gamma"), proj("Delta")];

it("affiche 3 tuiles au plus, et le nombre total dans le lien", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  render(<ProjectsCard />);
  expect(await screen.findAllByRole("article")).toHaveLength(3);
  expect(screen.getByRole("button", { name: "Tous les projets (4)" })).toBeTruthy();
});

it("épinglés d'abord", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue([proj("Alpha"), proj("Beta"), proj("Gamma"), proj("Delta", { pinned: true, last_activity_ms: 1 })]);
  render(<ProjectsCard />);
  expect((await screen.findAllByRole("article"))[0].getAttribute("aria-label")).toContain("Delta");
});

it("Reprendre et Orchestr. appellent resume avec le bon mode", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  const resume = vi.spyOn(projectsIpc, "resume").mockResolvedValue(null);
  render(<ProjectsCard />);
  const a = (await screen.findAllByRole("article"))[0];
  await userEvent.click(within(a).getByRole("button", { name: /^Reprendre/ }));
  expect(resume).toHaveBeenLastCalledWith("/P/Alpha", "normal");
  await userEvent.click(within(a).getByRole("button", { name: /Orchestr/ }));
  expect(resume).toHaveBeenLastCalledWith("/P/Alpha", "orchestrator");
});

it("affiche le message de reprise manuelle", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  vi.spyOn(projectsIpc, "resume").mockResolvedValue("Lance Claude à la main");
  render(<ProjectsCard />);
  await userEvent.click(within((await screen.findAllByRole("article"))[0]).getByRole("button", { name: /^Reprendre/ }));
  expect((await screen.findByRole("status")).textContent).toContain("Lance Claude à la main");
});

it("sans racine : état vide, saisie et Parcourir enregistrent les racines", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue([]);
  withRoots([]);
  const set = vi.spyOn(projectsIpc, "setRoots").mockResolvedValue();
  vi.spyOn(projectsIpc, "browse").mockResolvedValue(String.raw`D:\Code`);
  render(<ProjectsCard />);
  expect(await screen.findByText("Choisis le dossier où sont tes projets")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Parcourir" }));
  expect(set).toHaveBeenLastCalledWith([String.raw`D:\Code`]);
  await userEvent.type(screen.getByRole("textbox", { name: /dossier/i }), String.raw`E:\Dev` + "{Enter}");
  expect(set).toHaveBeenLastCalledWith([String.raw`E:\Dev`]);
});

it("racines définies mais aucun projet : message et sélecteur à la demande", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue([]);
  render(<ProjectsCard />);
  expect(await screen.findByText("Aucun projet trouvé")).toBeTruthy();
  expect(screen.queryByText("Choisis le dossier où sont tes projets")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Changer de dossier" }));
  expect(screen.getByText("Choisis le dossier où sont tes projets")).toBeTruthy();
});

it("rien n'est affiché (pas de sélecteur) tant que liste et réglages ne sont pas chargés", async () => {
  vi.spyOn(projectsIpc, "list").mockReturnValue(new Promise(() => {}));
  withRoots([]);
  render(<ProjectsCard />);
  await Promise.resolve();
  expect(screen.queryByText("Choisis le dossier où sont tes projets")).toBeNull();
  expect(screen.queryByText("Aucun projet trouvé")).toBeNull();
});

it("un dossier refusé par le backend affiche son message", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue([]);
  withRoots([]);
  vi.spyOn(projectsIpc, "setRoots").mockRejectedValue("Ce dossier n'existe pas.");
  render(<ProjectsCard />);
  await userEvent.type(await screen.findByRole("textbox", { name: /dossier/i }), "/nope{Enter}");
  expect((await screen.findByRole("alert")).textContent).toBe("Ce dossier n'existe pas.");
});

it("« Ouvrir le dossier » ouvre le chemin du projet", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  const openDir = vi.spyOn(projectsIpc, "openDir").mockResolvedValue(undefined);
  render(<ProjectsCard />);
  await userEvent.click(within((await screen.findAllByRole("article"))[0]).getByRole("button", { name: /Options/ }));
  await userEvent.click(screen.getByRole("button", { name: "Ouvrir le dossier" }));
  expect(openDir).toHaveBeenCalledWith("/P/Alpha");
});

it("« Tous les projets » met le focus dans la palette avec la portée p", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  const fp = vi.spyOn(focus, "focusPalette").mockImplementation(() => true);
  render(<ProjectsCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Tous les projets (4)" }));
  expect(fp).toHaveBeenCalledWith("p ");
});

it("une reprise qui échoue affiche l'erreur et réactive les boutons", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  vi.spyOn(projectsIpc, "resume").mockRejectedValue("échec : boom");
  render(<ProjectsCard />);
  const a = (await screen.findAllByRole("article"))[0];
  await userEvent.click(within(a).getByRole("button", { name: /^Reprendre/ }));
  expect((await screen.findByRole("alert")).textContent).toBe("Action impossible pour le moment.");
  expect((within(a).getByRole("button", { name: /^Reprendre/ }) as HTMLButtonElement).disabled).toBe(false);
});

it("« Tu en étais à » : plan, sinon session, sinon rien de récent", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  render(<ProjectsCard />);
  const [a, b, c] = await screen.findAllByRole("article");
  expect(a.textContent).toContain("Tu en étais à : l'heuristique octile");
  expect(b.textContent).toContain("Dernière session : signature des builds");
  expect(c.textContent).toContain("Rien de récent");
});

it("anneau, carte d'activité et puces accessibles", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue([proj("Alpha", { plan, dirty: 4, branch: "feat/x", activity_14d: [1, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3] }), proj("Beta")]);
  render(<ProjectsCard />);
  const [a, b] = await screen.findAllByRole("article");
  expect(within(a).getByRole("img", { name: "étape 7 sur 12" }).textContent).toBe("7/12");
  expect(a.textContent).toContain("⑂ feat/x");
  expect(a.textContent).toContain("4 modifs");
  expect(a.textContent).toContain("activité : 3 jours sur 14");
  expect(b.textContent).toContain("propre");
  expect(within(b).getByText("—")).toBeTruthy();
});

it("menu : épingler, masquer", async () => {
  vi.spyOn(projectsIpc, "list").mockResolvedValue(four());
  const pin = vi.spyOn(projectsIpc, "pin").mockResolvedValue();
  const hide = vi.spyOn(projectsIpc, "hide").mockResolvedValue();
  render(<ProjectsCard />);
  const a = (await screen.findAllByRole("article"))[0];
  await userEvent.click(within(a).getByRole("button", { name: /Options/ }));
  await userEvent.click(screen.getByRole("button", { name: "Épingler" }));
  expect(pin).toHaveBeenCalledWith("/P/Alpha", true);
  await userEvent.click(within(a).getByRole("button", { name: /Options/ }));
  await userEvent.click(screen.getByRole("button", { name: "Masquer" }));
  expect(hide).toHaveBeenCalledWith("/P/Alpha", true);
});

it("ago : français", () => {
  const n = 10 * DAY;
  expect(ago(n - 3 * DAY, n)).toBe("il y a 3 j");
  expect(ago(n - 5 * 60_000, n)).toBe("il y a 5 min");
  expect(ago(0, n)).toBe("aucune activité");
});
