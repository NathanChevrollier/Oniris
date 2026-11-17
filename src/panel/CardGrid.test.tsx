import { render, screen, cleanup } from "@testing-library/react";
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

it("chaque carte a un en-tête fixe et un corps défilant", () => {
  const { container } = render(<CardGrid slots={{}} />);
  for (const card of container.querySelectorAll(".grid>.card")) {
    expect(card.firstElementChild?.tagName).toBe("H3");
    expect(card.lastElementChild).toHaveClass("card-body", "scroll-area");
  }
});
