// Test de fuite longue durée (Node, sans dépendance). Voir README.md. Usage : node soak.mjs [--hours 24] [--every-min 5]
import { appendFileSync, writeFileSync } from "node:fs";
import { cpus } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { snapshot, sleep } from "./measure.mjs";
import { verdict } from "./leak.mjs";

const arg = (name, def) => { const i = process.argv.indexOf(name); return i > 0 ? Number(process.argv[i + 1]) : def; };
const hours = arg("--hours", 24), everyMin = arg("--every-min", 5);
if (!(hours > 0) || !(everyMin > 0)) { console.error("usage : node soak.mjs [--hours N] [--every-min M]"); process.exit(2); }
const file = join(dirname(fileURLToPath(import.meta.url)), `soak-${new Date().toISOString().slice(0, 10)}.csv`);
writeFileSync(file, "t_ms,ram_mb,cpu_pct,processes\n");

const samples = [];
const report = (partial) => {
  const v = verdict(samples);
  const tag = partial ? " (partiel)" : "";
  if (v.insufficient) console.log(`Verdict${tag} : pas assez de mesures après la 1re heure exclue (${samples.length} échantillons au total).`);
  else console.log(`Verdict${tag} : pente ${v.slopeMbPerH.toFixed(2)} Mo/h sur ${v.used} échantillons -> ${v.leak ? "FUITE (> 1 Mo/h)" : "OK (pas de fuite)"}`);
  console.log(`-> ${file}`);
};
process.on("SIGINT", () => { report(true); process.exit(0); });

let prev = snapshot();
if (!prev.count) { console.error("oniris.exe introuvable : lancer l'app d'abord."); process.exit(1); }
const t0 = Date.now(), end = t0 + hours * 3_600_000, cores = cpus().length;
let prevT = t0;
while (Date.now() < end) {
  await sleep(Math.min(everyMin * 60_000, Math.max(0, end - Date.now())));
  const cur = snapshot(), now = Date.now();
  const cpuPct = ((cur.cpuSeconds - prev.cpuSeconds) / ((now - prevT) / 1000) / cores) * 100;
  const s = { t: now - t0, mb: cur.privateBytes / 1048576 };
  samples.push(s);
  appendFileSync(file, `${s.t},${s.mb.toFixed(1)},${cpuPct.toFixed(3)},${cur.count}\n`);
  prev = cur; prevT = now;
}
report(false);
