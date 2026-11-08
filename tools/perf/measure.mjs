// Banc de mesure Oniris (Node 24, sans dépendance). Voir README.md avant de lancer.
// Prérequis : app lancée avec WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { cpus } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isMain } from "./perf-lib.mjs";

const PORT = 9333;
export const BUDGET = { openP95Ms: 100, cpuPct: 0.5, ramMb: 200 };
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Somme RAM privée + temps CPU (s) d'oniris.exe et de tous ses descendants (msedgewebview2.exe...).
const PS = `
$all = Get-CimInstance Win32_Process
$ids = [System.Collections.Generic.HashSet[int]]::new()
$all | Where-Object { $_.Name -eq 'oniris.exe' } | ForEach-Object { [void]$ids.Add([int]$_.ProcessId) }
do { $n = $ids.Count
  $all | Where-Object { $ids.Contains([int]$_.ParentProcessId) } | ForEach-Object { [void]$ids.Add([int]$_.ProcessId) }
} while ($ids.Count -ne $n)
$p = Get-Process -Id @($ids) -ErrorAction SilentlyContinue
[pscustomobject]@{
  count = @($p).Count
  pids = @($ids)
  privateBytes = ($p | Measure-Object PrivateMemorySize64 -Sum).Sum
  cpuSeconds = ($p | ForEach-Object { $_.TotalProcessorTime.TotalSeconds } | Measure-Object -Sum).Sum
} | ConvertTo-Json -Compress`;
export const snapshot = () => {
  const out = execFileSync("powershell", ["-NoProfile", "-Command", PS], { encoding: "utf8" });
  return JSON.parse(out);
};

async function openCycles(n) {
  const targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
  const t = targets.find((x) => x.type === "page" && /panel/i.test(x.url)) ?? targets.find((x) => x.type === "page");
  if (!t) throw new Error("aucune page CDP (app lancée avec le port 9333 ?)");
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error("connexion CDP impossible")); });
  const reply = new Promise((res, rej) => {
    ws.onmessage = (m) => {
      const d = JSON.parse(m.data);
      if (d.id !== 1) return;
      d.error || d.result.exceptionDetails ? rej(new Error(JSON.stringify(d.error ?? d.result.exceptionDetails))) : res(d.result.result.value);
    };
  });
  ws.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: {
    expression: `window.__TAURI_INTERNALS__.invoke("perf_open_cycles", { n: ${n} })`,
    awaitPromise: true, returnByValue: true } }));
  try { return await reply; } finally { ws.close(); }
}

const row = (k, v, budget, ok, invalid = false) => console.log(`${k.padEnd(22)}${String(v).padEnd(12)}${budget.padEnd(10)}${invalid ? "INVALIDE" : ok ? "OK" : "DÉPASSÉ"}`);

// Ouverture (30 cycles) puis repos 2 x 60 s : RAM privée et CPU moyen. Écrit results-<date>.json.
export async function measure() {
  console.log("Ouverture : 30 cycles (le panneau va clignoter)...");
  const open = await openCycles(30);
  console.log("Repos 60 s, panneau fermé...");
  await sleep(60_000);
  const a = snapshot();
  await sleep(60_000);
  const b = snapshot();
  const cores = cpus().length;
  const cpuPct = ((b.cpuSeconds - a.cpuSeconds) / 60 / cores) * 100;
  const ramMb = b.privateBytes / 1048576;
  const result = { date: new Date().toISOString(), open, ramMb, cpuPct, cores, processes: b.count, budget: BUDGET };
  const file = join(dirname(fileURLToPath(import.meta.url)), `results-${result.date.slice(0, 10)}.json`);
  writeFileSync(file, JSON.stringify(result, null, 2));
  return { ...result, file, pids: b.pids };
}

// 0 échantillon (jamais armé / tout en timeout) ou > 10 % de timeouts : la mesure ne vaut rien.
export const isInvalid = (open) => open.samples === 0 || open.timeouts > 3;

if (isMain(import.meta.url, process.argv[1])) {
  const { open, cpuPct, ramMb, file } = await measure();
  console.log(`
${"Mesure".padEnd(22)}${"Valeur".padEnd(12)}${"Budget".padEnd(10)}`);
  const invalid = isInvalid(open);
  row("Ouverture p95 (ms)", open.p95_ms.toFixed(1), `< ${BUDGET.openP95Ms}`, open.p95_ms < BUDGET.openP95Ms, invalid);
  row("Timeouts (/30)", open.timeouts, "<= 3", open.timeouts <= 3, invalid);
  row("CPU repos (%)", cpuPct.toFixed(3), `< ${BUDGET.cpuPct}`, cpuPct < BUDGET.cpuPct);
  row("RAM privée (Mo)", ramMb.toFixed(0), `< ${BUDGET.ramMb}`, ramMb < BUDGET.ramMb);
  console.log(`échantillons ${open.samples}, timeouts ${open.timeouts}, p50 ${open.p50_ms} ms, max ${open.max_ms} ms
-> ${file}`);
}
