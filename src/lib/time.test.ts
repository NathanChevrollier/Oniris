import { it, expect } from "vitest";
import { nowPosition, greeting, formatLongDate, hhmm } from "./time";

it("nowPosition", () => {
  expect(nowPosition(new Date(2026, 10, 3, 8, 0), 8, 23)).toBe(0);
  expect(nowPosition(new Date(2026, 10, 3, 15, 30), 8, 23)).toBeCloseTo(0.5);
  expect(nowPosition(new Date(2026, 10, 3, 6, 0), 8, 23)).toBe(0);
  expect(nowPosition(new Date(2026, 10, 3, 23, 59), 8, 23)).toBe(1);
});
it("greeting", () => {
  expect(greeting(new Date(2026, 10, 3, 9), "Nathan")).toBe("Bonjour Nathan");
  expect(greeting(new Date(2026, 10, 3, 19), "")).toBe("Bonsoir");
  expect(greeting(new Date(2026, 10, 3, 17, 59), "  ")).toBe("Bonjour");
  expect(greeting(new Date(2026, 10, 3, 18), "Léa")).toBe("Bonsoir Léa");
});
it("formatLongDate", () => { expect(formatLongDate(new Date(2026, 10, 3))).toBe("Mardi 3 novembre"); });
it("hhmm", () => { expect(hhmm(new Date(2026, 10, 3, 8, 5))).toBe("08:05"); });
