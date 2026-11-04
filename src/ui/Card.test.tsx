import { render, screen } from "@testing-library/react";
import { it, expect } from "vitest";
import { Card } from "./Card";

it("Card affiche le titre en majuscules via CSS, pas dans le texte", () => {
  render(<Card title="Mails" span={5}>x</Card>);
  expect(screen.getByRole("heading", { name: "Mails" })).toBeInTheDocument();
});

it("Card affiche un aside valant 0 dans son emplacement", () => {
  const { container } = render(<Card title="Mails" aside={0} span={5} />);
  expect(container.querySelector(".r")).toHaveTextContent("0");
});
