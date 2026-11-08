// Verdict des budgets §9 (Node, sans dépendance). Lance measure.mjs puis ajoute VRAM et processus voix/IA.
// Voir README.md : à lancer par l'utilisateur, jamais automatiquement.
import { execFileSync } from "node:child_process";
import { BUDGET, measure, isInvalid } from "./measure.mjs";

const ps = (cmd) => execFileSync("powershell", ["-NoProfile", "-Command", cmd], { encoding: "utf8" }).trim();

// VRAM dédiée (Mo) sommée sur les pid donnés ; null si le compteur n'existe pas (pilote, Windows).
function vramMb(pids) {
  const cmd = `$sum = 0; $found = $false
foreach ($id in @(${pids.join(",")})) {
  try { $c = Get-Counter "\GPU Process Memory(pid_$id*)\Dedicated Usage" -ErrorAction Stop
    $found = $true; $sum += ($c.CounterSamples | Measure-Object CookedValue -Sum).Sum } catch {}
}
if ($found) { $sum } else { 'null' }`;
  const out = ps(cmd);
  return out === "null" || out === "" ? null : Number(out) / 1048576;
}

// Processus de voix/IA vivants (au repos : 0). Le service ollama seul ne compte pas, seulement son runner.
function voiceAiCount() {
  const out = ps(`@(Get-CimInstance Win32_Process | Where-Object {
  $_.Name -in 'whisper-server.exe','piper.exe','ollama_llama_server.exe','claude.exe' -or
  ($_.Name -eq 'ollama.exe' -and $_.CommandLine -match 'runner') }).Count`);
  return Number(out);
}

const row = (k, v, budget, ok) => console.log(`${k.padEnd(24)}${String(v).padEnd(14)}${budget.padEnd(10)}${ok === null ? "NON MESURABLE" : ok ? "OK" : "HORS BUDGET"}`);

const r = await measure();
const vram = vramMb(r.pids);
const ai = voiceAiCount();
const invalid = isInvalid(r.open);
console.log(`\n${"Mesure".padEnd(24)}${"Valeur".padEnd(14)}${"Budget".padEnd(10)}Verdict`);
row("Ouverture p95 (ms)", r.open.p95_ms.toFixed(1), `< ${BUDGET.openP95Ms}`, invalid ? null : r.open.p95_ms < BUDGET.openP95Ms);
row("CPU repos (%)", r.cpuPct.toFixed(3), `< ${BUDGET.cpuPct}`, r.cpuPct < BUDGET.cpuPct);
row("RAM privée (Mo)", r.ramMb.toFixed(0), `< ${BUDGET.ramMb}`, r.ramMb < BUDGET.ramMb);
row("VRAM (Mo)", vram === null ? "non mesurable" : vram.toFixed(0), "0", vram === null ? null : vram === 0);
row("Processus voix/IA", ai, "0", ai === 0);
if (invalid) console.log("Mesure d'ouverture INVALIDE (0 échantillon ou > 3 timeouts) : ne rien conclure.");
console.log(`-> ${r.file}`);
