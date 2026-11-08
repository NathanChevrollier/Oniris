// Mesure de mise en page du panneau (Node 24, sans dépendance) : Edge headless contre un serveur Vite déjà lancé.
// Usage : node tools/perf/layout.mjs <url-panel.html> [--shots <dossier>] [--edge <msedge.exe>]
//   ex. : pnpm vite --port 5199 &  puis  node tools/perf/layout.mjs http://127.0.0.1:5199/panel.html --shots .superpowers/sdd/x
// Hors Tauri le panneau s'affiche ouvert avec ses données factices : rien n'est lu sur la machine.
// Écrit tools/perf/layout-<date>.json ; code de sortie 1 si une taille ne tient pas.
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const opt = (name, def) => (args.includes(name) ? args[args.indexOf(name) + 1] : def);
const url = args.find((a) => a.startsWith("http"));
if (!url) { console.error("usage : node tools/perf/layout.mjs <url panel.html> [--shots dossier] [--edge msedge.exe]"); process.exit(2); }
const edge = opt("--edge", "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe");
const shots = opt("--shots", null);
const only = opt("--only", null); // une seule taille, par son nom
const CDP_PORT = 9444;
const SIZES = [
  { name: "2560x1440", width: 2560, height: 1440, deviceScaleFactor: 1, required: true },
  { name: "1920x1080@1.25", width: 1920, height: 1080, deviceScaleFactor: 1.25, required: true },
  // 1080p physique à 125 % de mise à l'échelle Windows : informatif (864 px CSS de haut).
  { name: "1536x864@1.25", width: 1536, height: 864, deviceScaleFactor: 1.25, required: false },
];
const SHEETS = ["accounts", "iris", "journal", "settings"];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const MEASURE = `(() => {
  const g = document.querySelector(".inner > .grid"), inner = document.querySelector(".inner"), r = (e) => e.getBoundingClientRect();
  const gr = r(g);
  const cards = [...g.children].map((c) => {
    const cr = r(c);
    return { title: c.querySelector("h2,h3,.card-title")?.textContent ?? c.className, h: Math.round(cr.height),
      overflowsGrid: cr.bottom > gr.bottom + 1, overflowsSelf: c.scrollHeight > c.clientHeight + 1 };
  });
  return { viewport: { w: innerWidth, h: innerHeight, dpr: devicePixelRatio },
    grid: { client: g.clientHeight, scroll: g.scrollHeight, more: g.classList.contains("more") },
    inner: { client: inner.clientHeight, scroll: inner.scrollHeight }, cards };
})()`;

let proc, userDir;
const launch = () => {
  userDir = mkdtempSync(join(tmpdir(), "oniris-layout-"));
  proc = spawn(edge, ["--headless=new", `--remote-debugging-port=${CDP_PORT}`, `--user-data-dir=${userDir}`, "--no-first-run", "--disable-gpu", "about:blank"], { stdio: "ignore" });
};
const stop = async () => {
  ws?.close();
  if (proc?.pid) spawn("taskkill", ["/PID", String(proc.pid), "/T", "/F"], { stdio: "ignore" });
  await sleep(1500);
  rmSync(userDir, { recursive: true, force: true });
};
let ws, nextId = 0;
const pending = new Map();
const send = (method, params = {}) => new Promise((res, rej) => {
  const id = ++nextId;
  pending.set(id, { res, rej });
  ws.send(JSON.stringify({ id, method, params }));
});
const evaluate = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
};
const shot = async (file) => {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  const { data } = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(shots, file), Buffer.from(data, "base64"));
};

async function connect() {
  for (let i = 0; i < 50; i++) {
    try {
      const t = (await (await fetch(`http://127.0.0.1:${CDP_PORT}/json`)).json()).find((x) => x.type === "page");
      if (t) {
        ws = new WebSocket(t.webSocketDebuggerUrl);
        await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
        ws.onmessage = (m) => {
          const d = JSON.parse(m.data), p = pending.get(d.id);
          if (d.method === "Log.entryAdded") pageErrors.push(d.params.entry.text + " " + (d.params.entry.url ?? ""));
          if (d.method === "Runtime.exceptionThrown") pageErrors.push(d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text);
          if (p) { pending.delete(d.id); d.error ? p.rej(new Error(d.error.message)) : p.res(d.result); }
        };
        return;
      }
    } catch { /* Edge démarre */ }
    await sleep(200);
  }
  throw new Error("Edge headless injoignable");
}

const results = [];
const pageErrors = [];
try {
  for (const size of SIZES.filter((z) => !only || z.name === only)) {
    // Un Edge neuf par taille : changer de facteur d'échelle en cours de route laisse des mises en page périmées.
    launch();
    await connect();
    await send("Page.enable"); await send("Runtime.enable"); await send("Log.enable");
    await send("Emulation.setDeviceMetricsOverride", { width: size.width, height: size.height, deviceScaleFactor: size.deviceScaleFactor, mobile: false });
    const states = {};
    const fresh = async () => { const nav = await send("Page.navigate", { url }); if (nav.errorText) throw new Error(`navigation : ${nav.errorText}`);
      for (let i = 0; i < 60 && !(await evaluate(`!!document.querySelector(".inner > .grid")`)); i++) await sleep(250); // Vite transforme à froid
      if (!(await evaluate(`!!document.querySelector(".inner > .grid")`))) throw new Error(`panneau absent de ${url} : ${pageErrors.join(" | ") || "aucune erreur de page"} ; page : ${await evaluate("location.href + \" \" + document.readyState")}`);
      for (let i = 0; i < 20 && await evaluate(`[...document.querySelectorAll(".grid > *")].some((c) => c.getAnimations().some((a) => a.playState === "running"))`); i++) await sleep(250); // montée `rise` des cartes
      await sleep(300); };
    const record = async (state) => { states[state] = await evaluate(MEASURE); await shot(`layout-${size.name}-${state}.png`); };
    await fresh(); await record("repos");
    // Conversation : un message à IRIS (sans Rust, le tour reste « en réflexion »).
    await fresh();
    await evaluate(`document.querySelector("input").focus()`);
    await send("Input.insertText", { text: "Quel est mon programme ?" });
    for (const type of ["keyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, text: "\r" });
    await sleep(600); await record("conversation");
    for (const sheet of SHEETS) {
      await fresh();
      await evaluate(`document.querySelector('.hdr-btn[data-sheet="${sheet}"]').click()`);
      await sleep(800); await record(`feuille-${sheet}`);
    }
    const rest = states.repos;
    const ok = !rest.grid.more && rest.grid.client === rest.grid.scroll && rest.inner.client >= rest.inner.scroll
      && rest.cards.every((c) => !c.overflowsGrid && !c.overflowsSelf);
    results.push({ ...size, ok, states });
    console.log(`${size.name.padEnd(16)} grille ${rest.grid.client}/${rest.grid.scroll}  .more=${rest.grid.more}  ${ok ? "OK" : "NE TIENT PAS"}${size.required ? "" : " (informatif)"}`);
    await stop();
  }
} finally {
  await stop();
}
const date = new Date().toISOString().slice(0, 10);
const out = join(dirname(fileURLToPath(import.meta.url)), `layout-${date}.json`);
writeFileSync(out, JSON.stringify({ date, url, sizes: results }, null, 2));
console.log("→", out);
process.exit(results.filter((r) => r.required).every((r) => r.ok) ? 0 : 1);
