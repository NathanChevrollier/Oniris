import { render, screen } from "@testing-library/react";
import { it, expect } from "vitest";
import { Card } from "./Card";

it("Card affiche le titre en majuscules via CSS, pas dans le texte", () => {
  render(<Card title="Mails" span={5}>x</Card>);
  expect(screen.getByRole("heading", { name: "Mails" })).toBeInTheDocument();
});
