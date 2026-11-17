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
  const m = (base: number, app = 0) => ({ base, app, tone: "sombre" as const });
  expect(glassAlpha("manual", 58, m(0.9))).toBe(0.58);
  expect(glassAlpha("auto", 58, null)).toBe(0.58);
  expect(glassAlpha("auto", 58, m(0.3))).toBeCloseTo(0.3);
  // préférence : décalage autour de 58, borné
  expect(glassAlpha("auto", 95, m(0.3))).toBeCloseTo(0.522);
  expect(glassAlpha("auto", 20, m(0.3))).toBe(0.25);
  // application derrière (VS Code mesuré à 0,55) : nettement plus opaque, sauf réglage coupé
  expect(glassAlpha("auto", 58, m(0.55, 1))).toBeCloseTo(0.85);
  expect(glassAlpha("auto", 58, m(0.3, 0.5))).toBeCloseTo(0.7);
  expect(glassAlpha("auto", 58, m(0.55, 1), false)).toBeCloseTo(0.55);
  expect(glassAlpha("auto", 58, m(0.94, 1))).toBe(0.96);
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
