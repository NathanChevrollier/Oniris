// Verdict des budgets §9 (Node, sans dépendance). Lance measure.mjs puis ajoute VRAM et processus voix/IA.
// Voir README.md : à lancer par l'utilisateur, jamais automatiquement.
import { execFileSync } from "node:child_process";
import { BUDGET, measure, isInvalid } from "./measure.mjs";
import { vramScript } from "./perf-lib.mjs";

const ps = (cmd) => execFileSync("powershell", ["-NoProfile", "-Command", cmd], { encoding: "utf8" }).trim();

const row = (k, v, budget, ok) => console.log(`${k.padEnd(24)}${String(v).padEnd(14)}${budget.padEnd(10)}${ok === null ? "NON MESURABLE" : ok ? "OK" : "HORS BUDGET"}`);

const r = await measure();
const g = JSON.parse(ps(vramScript(r.pids)));
const mb = (x) => (g.has ? x / 1048576 : null);
const vram = mb(g.aiVram), ai = g.aiCount;
const invalid = isInvalid(r.open);
console.log(`\n${"Mesure".padEnd(24)}${"Valeur".padEnd(14)}${"Budget".padEnd(10)}Verdict`);
row("Ouverture p95 (ms)", r.open.p95_ms.toFixed(1), `< ${BUDGET.openP95Ms}`, invalid ? null : r.open.p95_ms < BUDGET.openP95Ms);
row("CPU repos (%)", r.cpuPct.toFixed(3), `< ${BUDGET.cpuPct}`, r.cpuPct < BUDGET.cpuPct);
row("RAM privée (Mo)", r.ramMb.toFixed(0), `< ${BUDGET.ramMb}`, r.ramMb < BUDGET.ramMb);
row("VRAM voix/IA (Mo)", vram === null ? "non mesurable" : vram.toFixed(0), "0", vram === null ? null : vram === 0);
row("Processus voix/IA", ai, "0", ai === 0);
console.log(`VRAM Oniris + WebView2 (informatif, sans budget) : ${g.has ? mb(g.onirisVram).toFixed(0) + " Mo" : "non mesurable"}`);
if (invalid) console.log("Mesure d'ouverture INVALIDE (0 échantillon ou > 3 timeouts) : ne rien conclure.");
console.log(`-> ${r.file}`);
