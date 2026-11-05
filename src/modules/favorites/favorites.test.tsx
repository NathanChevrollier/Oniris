import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { FavoritesCard, hueOf } from "./FavoritesCard";
import { favoritesIpc, importIpc, formatReport, type Favorite } from "./api";
import { contrastRatio, hslToRgb, SAT, LIGHT, DARK } from "./contrast";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => {
  usePanelStore.setState({ open: true });
  // jsdom n'a pas PointerEvent : MouseEvent porte clientX/clientY.
  (window as unknown as { PointerEvent: unknown }).PointerEvent ??= MouseEvent;
  // Tuiles de 50 px alignées sur une ligne : la tuile i occupe [50i, 50i+50).
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const i = Number((this as HTMLElement).dataset.index ?? -1);
    return i < 0 ? new DOMRect() : new DOMRect(i * 50, 0, 50, 60);
  });
});

const fav = (id: number, title: string, category: string | null = null): Favorite =>
  ({ id, title, url: `https://${title.toLowerCase()}.com`, category, position: id, icon: null });
const three = [fav(1, "Alpha"), fav(2, "Beta"), fav(3, "Gamma")];
const ok = { Done: { action_id: 1, output: {} } };

it("un clic ouvre le favori", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const open = vi.spyOn(favoritesIpc, "open").mockResolvedValue(ok);
  render(<FavoritesCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Beta" }));
  expect(open).toHaveBeenCalledWith(2);
});

it("le glisser-déposer appelle reorder avec le nouvel ordre, sans ouvrir", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const open = vi.spyOn(favoritesIpc, "open").mockResolvedValue(ok);
  const reorder = vi.spyOn(favoritesIpc, "reorder").mockResolvedValue();
  render(<FavoritesCard />);
  const alpha = await screen.findByRole("button", { name: "Alpha" });
  fireEvent.pointerDown(alpha, { clientX: 25, clientY: 20, button: 0, pointerId: 1 });
  fireEvent.pointerMove(alpha, { clientX: 60, clientY: 22, pointerId: 1 }); // > 4 px : le glisser démarre
  fireEvent.pointerMove(alpha, { clientX: 130, clientY: 22, pointerId: 1 }); // au-dessus de Gamma
  fireEvent.pointerUp(alpha, { clientX: 130, clientY: 22, pointerId: 1 });
  fireEvent.click(alpha);
  expect(reorder).toHaveBeenCalledWith([2, 3, 1]);
  expect(open).not.toHaveBeenCalled();
});

it("Alt+flèche déplace le favori au clavier", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const reorder = vi.spyOn(favoritesIpc, "reorder").mockResolvedValue();
  render(<FavoritesCard />);
  const beta = await screen.findByRole("button", { name: "Beta" });
  beta.focus();
  await userEvent.keyboard("{Alt>}{ArrowLeft}{/Alt}");
  expect(reorder).toHaveBeenLastCalledWith([2, 1, 3]);
  await userEvent.keyboard("{Alt>}{ArrowRight}{ArrowRight}{/Alt}");
  expect(reorder).toHaveBeenLastCalledWith([1, 3, 2]);
});

it("titres de catégorie seulement s'il y en a plusieurs", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue([fav(1, "Alpha", "Outils"), fav(2, "Beta", "Outils")]);
  const { unmount } = render(<FavoritesCard />);
  await screen.findByRole("button", { name: "Alpha" });
  expect(screen.queryByRole("heading", { level: 4 })).toBeNull();
  unmount();
  vi.spyOn(favoritesIpc, "list").mockResolvedValue([fav(1, "Alpha", "Outils"), fav(2, "Beta", "Sites")]);
  render(<FavoritesCard />);
  expect(await screen.findByRole("heading", { level: 4, name: "Sites" })).toBeInTheDocument();
});

it("initiales sur les deux premiers mots, favicon quand il existe", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue([fav(1, "Nexus Dashboard"), { ...fav(2, "Beta"), icon: "data:image/png;base64,AAAA" }]);
  render(<FavoritesCard />);
  expect(await screen.findByText("ND")).toBeInTheDocument();
  expect(document.querySelector("img")).toHaveAttribute("src", "data:image/png;base64,AAAA");
});

it("le formulaire « + » ajoute, le menu renomme et supprime", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const add = vi.spyOn(favoritesIpc, "add").mockResolvedValue(ok);
  const rename = vi.spyOn(favoritesIpc, "rename").mockResolvedValue();
  const remove = vi.spyOn(favoritesIpc, "remove").mockResolvedValue(ok);
  render(<FavoritesCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Ajouter un favori" }));
  await userEvent.type(screen.getByLabelText("Adresse du site"), "https://x.org");
  await userEvent.type(screen.getByLabelText("Titre (facultatif)"), "Xo{Enter}");
  expect(add).toHaveBeenCalledWith("https://x.org", "Xo");

  fireEvent.contextMenu(screen.getByRole("button", { name: "Alpha" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Renommer" }));
  const input = screen.getByLabelText("Nouveau titre");
  await userEvent.clear(input);
  await userEvent.type(input, "Omega{Enter}");
  expect(rename).toHaveBeenCalledWith(1, "Omega");

  fireEvent.contextMenu(screen.getByRole("button", { name: "Beta" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Supprimer" }));
  expect(remove).toHaveBeenCalledWith(2);
});

it("état vide : invitation et bouton Importer actif", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue([]);
  render(<FavoritesCard />);
  expect(await screen.findByText(/Ajoute tes sites favoris/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Importer" })).toBeEnabled();
});

it("le relâchement hors de la tuile termine le glisser (capture du pointeur)", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const reorder = vi.spyOn(favoritesIpc, "reorder").mockResolvedValue();
  const capture = vi.fn(), release = vi.fn();
  Element.prototype.setPointerCapture = capture;
  Element.prototype.releasePointerCapture = release;
  render(<FavoritesCard />);
  const alpha = await screen.findByRole("button", { name: "Alpha" });
  fireEvent.pointerDown(alpha, { clientX: 25, clientY: 20, button: 0, pointerId: 7 });
  fireEvent.pointerMove(alpha, { clientX: 60, clientY: 22, pointerId: 7 });
  expect(capture).toHaveBeenCalledWith(7);
  expect(alpha.closest("li")).toHaveClass("dragging");
  fireEvent.pointerMove(alpha, { clientX: 130, clientY: 22, pointerId: 7 });
  fireEvent.pointerUp(document.body, { clientX: 900, clientY: 900, pointerId: 7 }); // hors de la tuile
  expect(alpha.closest("li")).not.toHaveClass("dragging");
  expect(release).toHaveBeenCalledWith(7);
  expect(reorder).toHaveBeenCalledWith([2, 3, 1]);
  fireEvent.pointerMove(alpha, { clientX: 130, clientY: 22, pointerId: 7 }); // plus de glisser en cours
  expect(alpha.closest("li")).not.toHaveClass("dragging");
});

it("annonce la nouvelle position, le menu se ferme sur Échap et au clic dehors", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  vi.spyOn(favoritesIpc, "reorder").mockResolvedValue();
  render(<FavoritesCard />);
  const beta = await screen.findByRole("button", { name: "Beta" });
  beta.focus();
  await userEvent.keyboard("{Alt>}{ArrowRight}{/Alt}");
  expect(screen.getByRole("status")).toHaveTextContent("Déplacé en position 3 sur 3");
  expect(screen.getByRole("list", { name: "Favoris" })).toBeInTheDocument();
  fireEvent.contextMenu(beta);
  expect(screen.getByRole("menu")).toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
  fireEvent.contextMenu(beta);
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("menu")).toBeNull();
});

it("un renommage qui échoue affiche une erreur", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  vi.spyOn(favoritesIpc, "rename").mockRejectedValue("boom");
  render(<FavoritesCard />);
  fireEvent.contextMenu(await screen.findByRole("button", { name: "Alpha" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Renommer" }));
  await userEvent.type(screen.getByLabelText("Nouveau titre"), "x{Enter}");
  expect(await screen.findByRole("alert")).toHaveTextContent("Renommage impossible");
});

it("texte blanc sur le dégradé : contraste >= 4,5 sur 12 teintes, au bord clair comme au bord sombre", () => {
  for (let h = 0; h < 360; h += 30) {
    for (const l of [LIGHT, DARK]) expect(contrastRatio([255, 255, 255], hslToRgb(h, SAT, l)), `teinte ${h}, l ${l}`).toBeGreaterThanOrEqual(4.5);
  }
  expect(contrastRatio([0, 0, 0], [255, 255, 255])).toBeCloseTo(21, 0);
});

it("la teinte reste dans 0..359", () => {
  for (const host of ["a.com", "github.com", "nexus.example.com", "zzzzzzzzzzzzzzzzzzzz.org", "é.fr", "", "x".repeat(500)]) {
    const h = hueOf(host);
    expect(Number.isInteger(h) && h >= 0 && h < 360, host).toBe(true);
  }
});

it("l'écouteur de fenêtre retiré est bien celui qui a été ajouté", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const add = vi.spyOn(window, "addEventListener");
  const rem = vi.spyOn(window, "removeEventListener");
  const { unmount } = render(<FavoritesCard />);
  const alpha = await screen.findByRole("button", { name: "Alpha" });
  fireEvent.pointerDown(alpha, { clientX: 25, clientY: 20, button: 0, pointerId: 1 });
  const added = add.mock.calls.find((c) => c[0] === "pointerup")![1];
  fireEvent.pointerUp(alpha, { pointerId: 1 });
  expect(rem.mock.calls.some((c) => c[0] === "pointerup" && c[1] === added)).toBe(true);
  fireEvent.pointerDown(alpha, { clientX: 25, clientY: 20, button: 0, pointerId: 1 });
  rem.mockClear();
  unmount(); // démontage en plein appui
  expect(rem.mock.calls.some((c) => c[0] === "pointerup" && c[1] === added)).toBe(true);
});

it("reorder refusé : message inline, ordre d'origine rétabli", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  vi.spyOn(favoritesIpc, "reorder").mockRejectedValue("ordre incomplet ou inconnu");
  render(<FavoritesCard />);
  const beta = await screen.findByRole("button", { name: "Beta" });
  beta.focus();
  await userEvent.keyboard("{Alt>}{ArrowLeft}{/Alt}");
  expect(await screen.findByRole("alert")).toHaveTextContent("Liste modifiée ailleurs — réessaie");
  expect(screen.getAllByRole("listitem").map((li) => li.textContent?.slice(0, 1))[0]).toBe("A");
});

it("Échap rend le focus au bouton « … » ; titres de liste par catégorie", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue([fav(1, "Alpha", "Outils"), fav(2, "Beta", "Sites")]);
  render(<FavoritesCard />);
  expect(await screen.findByRole("list", { name: "Favoris — Sites" })).toBeInTheDocument();
  fireEvent.contextMenu(screen.getByRole("button", { name: "Alpha" }));
  await userEvent.keyboard("{Escape}");
  expect(screen.getByRole("button", { name: "Options de Alpha" })).toHaveFocus();
});

it("après Supprimer, le focus va à la tuile suivante, sinon la précédente, sinon « + »", async () => {
  const cases: [Favorite[], number, string][] = [
    [three, 2, "Gamma"], // suivante
    [three, 3, "Beta"], // pas de suivante : précédente
    [[fav(1, "Alpha")], 1, "Ajouter un favori"], // liste vide : « + »
  ];
  for (const [items, id, expected] of cases) {
    vi.spyOn(favoritesIpc, "list").mockResolvedValue(items);
    vi.spyOn(favoritesIpc, "remove").mockResolvedValue(ok);
    const { unmount } = render(<FavoritesCard />);
    const title = items.find((f) => f.id === id)!.title;
    fireEvent.contextMenu(await screen.findByRole("button", { name: title }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Supprimer" }));
    await waitFor(() => expect(screen.getByRole("button", { name: expected })).toHaveFocus());
    unmount();
    vi.restoreAllMocks();
    usePanelStore.setState({ open: true });
  }
});

it("un échec de reorder autre que l'ordre strict : message générique et rechargement", async () => {
  const list = vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  vi.spyOn(favoritesIpc, "reorder").mockRejectedValue("échec : base verrouillée");
  render(<FavoritesCard />);
  const beta = await screen.findByRole("button", { name: "Beta" });
  beta.focus();
  list.mockClear();
  await userEvent.keyboard("{Alt>}{ArrowLeft}{/Alt}");
  expect(await screen.findByRole("alert")).toHaveTextContent("Réorganisation impossible");
  expect(list).toHaveBeenCalled();
});

it("« Ajouter » et « Renommer » effacent les erreurs périmées", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  vi.spyOn(favoritesIpc, "rename").mockRejectedValue("boom");
  render(<FavoritesCard />);
  fireEvent.contextMenu(await screen.findByRole("button", { name: "Alpha" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Renommer" }));
  await userEvent.type(screen.getByLabelText("Nouveau titre"), "x{Enter}");
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Ajouter un favori" }));
  expect(screen.queryByRole("alert")).toBeNull();

  fireEvent.contextMenu(screen.getByRole("button", { name: "Beta" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Renommer" }));
  await userEvent.type(screen.getByLabelText("Nouveau titre"), "y{Enter}");
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  fireEvent.contextMenu(screen.getByRole("button", { name: "Gamma" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Renommer" }));
  expect(screen.queryByRole("alert")).toBeNull();
});

it("le bouton Importer ouvre un menu et affiche le compte rendu par navigateur", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const browsers = vi.spyOn(importIpc, "browsers").mockResolvedValue([["Chrome", { added: 23, skipped_dup: 4, skipped_unsafe: 1 }], ["Firefox", { added: 1, skipped_dup: 0, skipped_unsafe: 0 }]]);
  render(<FavoritesCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Importer" }));
  expect(screen.getByRole("menuitem", { name: "Depuis un export Nexus…" })).toBeTruthy();
  await userEvent.click(screen.getByRole("menuitem", { name: "Depuis les navigateurs" }));
  expect(browsers).toHaveBeenCalled();
  expect(await screen.findByText("Chrome : 23 ajoutés · 4 déjà présents · 1 ignoré (lien non sûr)")).toBeTruthy();
  expect(screen.getByText("Firefox : 1 ajouté")).toBeTruthy();
  expect(screen.queryByRole("menu")).toBeNull();
});

it("l'import d'un export Nexus : annulation silencieuse, erreur en clair", async () => {
  vi.spyOn(favoritesIpc, "list").mockResolvedValue(three);
  const file = vi.spyOn(importIpc, "nexusFile").mockResolvedValueOnce(null).mockRejectedValueOnce("ce fichier n'est pas un export Nexus");
  render(<FavoritesCard />);
  for (let i = 0; i < 2; i++) {
    await userEvent.click(await screen.findByRole("button", { name: "Importer" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Depuis un export Nexus…" }));
  }
  expect((await screen.findByRole("alert")).textContent).toBe("ce fichier n'est pas un export Nexus");
  expect(file).toHaveBeenCalledTimes(2);
});

it("formatReport : pluriels et zéros omis", () => {
  expect(formatReport({ added: 0, skipped_dup: 0, skipped_unsafe: 0 })).toBe("0 ajouté");
  expect(formatReport({ added: 2, skipped_dup: 1, skipped_unsafe: 3 })).toBe("2 ajoutés · 1 déjà présent · 3 ignorés (lien non sûr)");
});
