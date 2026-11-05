import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { it, expect, afterEach } from "vitest";
import { CardGrid } from "./CardGrid";

afterEach(cleanup);

it("affiche l'emplacement fourni, « Bientôt » pour les autres, dans l'ordre", () => {
  render(<CardGrid slots={{ agenda: <p>MON AGENDA</p> }} />);
  expect(screen.getByText("MON AGENDA")).toBeTruthy();
  expect(screen.getAllByText("Bientôt")).toHaveLength(8);
  cleanup();
  const { container } = render(<CardGrid slots={{}} />);
  const titles = [...container.querySelectorAll(".card h3")].map((n) => n.textContent);
  const order = ["En lecture", "Mails", "Agenda", "Projets", "Ce PC", "VPS et favoris", "Notes", "Presse-papiers", "Minuteur"];
  const found = order.map((t) => titles.indexOf(t));
  expect(found.every((i) => i >= 0)).toBe(true);
  expect([...found].sort((a, b) => a - b)).toEqual(found);
});

it("fondu « more » seulement quand du contenu dépasse sous le bord visible", () => {
  const { container } = render(<CardGrid slots={{}} />);
  const grid = container.querySelector(".grid") as HTMLElement;
  const size = (scrollTop: number, clientHeight: number, scrollHeight: number) =>
    Object.defineProperties(grid, { scrollTop: { value: scrollTop, configurable: true }, clientHeight: { value: clientHeight, configurable: true }, scrollHeight: { value: scrollHeight, configurable: true } });
  size(0, 500, 900);
  fireEvent.scroll(grid);
  expect(grid).toHaveClass("more");
  size(400, 500, 900);
  fireEvent.scroll(grid);
  expect(grid).not.toHaveClass("more");
});

it("le fondu se recalcule à la fin de la montée des cartes (animationend)", () => {
  const { container } = render(<CardGrid slots={{}} />);
  const grid = container.querySelector(".grid") as HTMLElement;
  Object.defineProperties(grid, { scrollTop: { value: 0 }, clientHeight: { value: 500 }, scrollHeight: { value: 900, configurable: true } });
  fireEvent.animationEnd(grid.firstElementChild!);
  expect(grid).toHaveClass("more");
  Object.defineProperty(grid, "scrollHeight", { value: 500 });
  fireEvent.animationEnd(grid.firstElementChild!);
  expect(grid).not.toHaveClass("more");
});
