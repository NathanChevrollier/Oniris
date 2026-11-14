import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach } from "vitest";
import { CLOSE_OVERSHOOT, glassAlpha, MIN_PCT, releaseHeight } from "./surface";
import { JournalSheet } from "../modules/iris/JournalSheet";

afterEach(cleanup);

it("poignée : hauteur bornée, fermeture seulement au-delà du seuil", () => {
  const max = 1000, min = (max * MIN_PCT) / 100;
  expect(releaseHeight(600, 100, max)).toEqual({ close: false, pct: 70 });
  expect(releaseHeight(600, 900, max)).toEqual({ close: false, pct: 100 });
  // Sous le minimum mais dans la marge : on reste à la hauteur minimale.
  expect(releaseHeight(600, min - 600 - CLOSE_OVERSHOOT + 1, max)).toEqual({ close: false, pct: MIN_PCT });
  expect(releaseHeight(600, min - 600 - CLOSE_OVERSHOOT - 1, max)).toEqual({ close: true });
});

it("verre : manuel, ou mesuré en automatique (repli sur le manuel avant la première mesure)", () => {
  expect(glassAlpha("manual", 58, 1)).toBe(0.58);
  expect(glassAlpha("auto", 58, null)).toBe(0.58);
  expect(glassAlpha("auto", 58, 0)).toBeCloseTo(0.4);
  expect(glassAlpha("auto", 58, 1)).toBeCloseTo(0.88);
  expect(glassAlpha("auto", 58, 7)).toBeCloseTo(0.88);
});

it("feuille : un clic dans le vide la ferme, jamais un clic dans le contenu ni un glissé parti du contenu", async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  render(<JournalSheet open onClose={onClose} />);
  const sheet = screen.getByRole("dialog", { name: "Journal" });
  await user.click(screen.getByText("Aucune action pour l'instant."));
  await user.click(screen.getByRole("heading", { name: "Journal" }));
  expect(onClose).not.toHaveBeenCalled();
  // Sélection commencée dans le texte, relâchée dans le vide : le clic part sur un ancêtre commun.
  fireEvent.pointerDown(screen.getByText("Aucune action pour l'instant."));
  fireEvent.click(sheet);
  expect(onClose).not.toHaveBeenCalled();
  await user.click(sheet);
  expect(onClose).toHaveBeenCalledOnce();
});
