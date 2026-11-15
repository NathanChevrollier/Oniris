import { it, expect } from "vitest";
import { calc, convertMoney, convertUnits, evaluate, parseMoney } from "./calc";

it("priorités, parenthèses, puissance et unaire", () => {
  expect(evaluate("(12+8)*3")).toBe(60);
  expect(evaluate("2+3*4")).toBe(14);
  expect(evaluate("2^10")).toBe(1024);
  expect(evaluate("2^3^2")).toBe(512); // associatif à droite
  expect(evaluate("-2^2")).toBe(-4);
  expect(evaluate("10-4-3")).toBe(3);
  expect(evaluate("7/3")).toBeCloseTo(2.3333333);
  expect(evaluate("3 × 4 ÷ 2")).toBe(6);
  expect(evaluate("1,5*2")).toBe(3);
});

it("pourcentages", () => {
  expect(evaluate("15% de 240")).toBe(36);
  expect(evaluate("15 % of 240 + 4")).toBe(40);
  expect(evaluate("240 + 15%")).toBe(276);
  expect(evaluate("240 - 50%")).toBe(120);
  expect(evaluate("50%")).toBe(0.5);
});

it("erreurs et tentatives d'injection : rien n'est évalué", () => {
  for (const s of ["", "2+", "(1+2", "1/0", "alert(1)", "2+constructor", "process.exit()", "1;2", "`1`", "Math.PI", "de 3", "2 3"]) {
    expect(evaluate(s), s).toBeNull();
  }
});

it("calc : seulement pour un vrai calcul, copie sans séparateur", () => {
  expect(calc("2^10")).toEqual({ title: "= 1 024", subtitle: "2^10 · Entrée pour copier", copy: "1024" });
  expect(calc("7/3")?.copy).toBe("2,33333333333");
  for (const s of ["42", "-5", "2026-11-14", "rapport", "facture 2026"]) expect(calc(s), s).toBeNull();
});

it("conversions d'unités", () => {
  expect(convertUnits("5 km en mi")?.copy).toBe("3,10686");
  expect(convertUnits("100 °F en °C")?.copy).toBe("37,7778");
  expect(convertUnits("0 c to f")?.copy).toBe("32");
  expect(convertUnits("2 Go en Mo")?.copy).toBe("2048");
  expect(convertUnits("90 min en h")?.copy).toBe("1,5");
  expect(convertUnits("10 kg en lb")?.copy).toBe("22,0462");
  expect(convertUnits("5 km en kg")).toBeNull(); // grandeurs différentes
  expect(convertUnits("30 € en $")).toBeNull();
});

it("devises : lecture de la demande et conversion avec de faux taux", () => {
  expect(parseMoney("30 € en $")).toEqual({ amount: 30, from: "EUR", to: "USD" });
  expect(parseMoney("$12 en EUR")).toEqual({ amount: 12, from: "USD", to: "EUR" });
  expect(parseMoney("100 chf to gbp")).toEqual({ amount: 100, from: "CHF", to: "GBP" });
  for (const s of ["5 km en mi", "30 abc en xyz", "30 € en €", "$12 € en EUR", "30 en $"]) expect(parseMoney(s), s).toBeNull();
  const rates = { EUR: 1, USD: 2, GBP: 0.5 };
  expect(convertMoney({ amount: 30, from: "EUR", to: "USD" }, rates)?.copy).toBe("60,00");
  expect(convertMoney({ amount: 10, from: "USD", to: "GBP" }, rates)?.copy).toBe("2,50");
  expect(convertMoney({ amount: 10, from: "USD", to: "JPY" }, rates)).toBeNull();
});
