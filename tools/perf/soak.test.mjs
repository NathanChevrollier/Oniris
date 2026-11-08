import test from "node:test";
import assert from "node:assert/strict";
import { slope, verdict } from "./leak.mjs";

const H = 3_600_000;
const line = (perH, hours, stepMin = 5, base = 100, noise = () => 0) =>
  Array.from({ length: Math.floor((hours * 60) / stepMin) + 1 }, (_, i) => {
    const t = i * stepMin * 60_000;
    return { t, mb: base + (perH * t) / H + noise(i) };
  });

test("pente exacte", () => assert.ok(Math.abs(slope(line(2.5, 10)) - 2.5) < 1e-9));
test("pente nulle", () => assert.ok(Math.abs(slope(line(0, 5))) < 1e-9));
test("trop peu de points", () => assert.equal(slope([{ t: 0, mb: 1 }]), null));
test("données bruitées : pente retrouvée", () => {
  const s = slope(line(3, 24, 5, 100, (i) => (i % 2 ? 4 : -4)));
  assert.ok(Math.abs(s - 3) < 0.2, String(s));
});
test("fuite détectée > 1 Mo/h", () => assert.equal(verdict(line(2, 24)).leak, true));
test("stable : pas de fuite", () => assert.equal(verdict(line(0.2, 24)).leak, false));
test("la première heure est exclue", () => {
  // +200 Mo dans la 1re heure (chauffe), puis plat
  const s = line(0, 24).map((p) => ({ ...p, mb: p.mb + Math.min(p.t / H, 1) * 200 }));
  const v = verdict(s);
  assert.equal(v.leak, false);
  assert.ok(Math.abs(v.slopeMbPerH) < 1e-6);
  assert.equal(slope(s) > 1, true);
});
test("pas assez de données après exclusion", () => assert.equal(verdict(line(5, 1)).insufficient, true));
