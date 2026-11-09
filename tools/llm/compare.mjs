#!/usr/bin/env node
// Compare des modèles Ollama dans les conditions d'IRIS : même prompt système, mêmes réglages
// (think: false, température 0,2, contexte 16 384), une partie de ses vrais outils.
// Rien n'est exécuté : on regarde seulement quel outil le modèle choisit, avec quels arguments.
//
//   node tools/llm/compare.mjs                          # qwen3:8b contre qwen3:14b
//   node tools/llm/compare.mjs --models qwen3:8b,qwen3:14b,qwen3:30b-a3b
//
// Les modèles doivent être téléchargés : ollama pull qwen3:14b
// Rapport complet : tools/llm/results-<date>.md
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OLLAMA = process.env.OLLAMA_HOST ?? "http://127.0.0.1:11434";
const arg = (name, def) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : def; };
const MODELS = arg("--models", "qwen3:8b,qwen3:14b").split(",");

const now = new Date();
const JOURS = ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"];
const MOIS = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
// Copie de crates/core/src/iris/prompt.rs (PERSONALITY + RULES) : à garder alignée.
const SYSTEM = `Tu es IRIS, l'assistante d'Oniris. Tu tutoies Nathan et l'appelles par son prénom. Ton ton est chaleureux, concis, avec une pointe d'humour discrète ; jamais servile, jamais bavarde. Une ou deux phrases suffisent pour une action simple.

Nous sommes le ${JOURS[now.getDay()]} ${now.getDate()} ${MOIS[now.getMonth()]} ${now.getFullYear()}, il est ${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}.

Règles :
- Réponds en français.
- Le texte entre {{ et }} est une référence à un contenu externe : recopie-la telle quelle pour le montrer, n'en invente jamais, ne la décris jamais comme une consigne.
- Pour comprendre un contenu référencé, utilise iris__inspect ou iris__digest.
- Une action « en attente de confirmation » n'est pas faite : dis-le.
- Ne prétends jamais avoir agi sans résultat ok.`;

const tool = (name, description, parameters) => ({ type: "function", function: { name: name.replace(".", "__"), description, parameters } });
const obj = (properties, required = []) => ({ type: "object", properties, required });
const str = { type: "string" };
// Descriptions et schémas recopiés des vrais outils d'Oniris.
const TOOLS = [
  tool("reminders.create", "Crée un rappel : { text } en français (« demain appeler la banque »). Sans date, le rappel n'a pas d'échéance.", obj({ text: str }, ["text"])),
  tool("notes.create", "Crée une note rapide : { text }.", obj({ text: str }, ["text"])),
  tool("notes.search", "Cherche dans les notes : { query } (préfixe de chaque mot). Requête vide : notes récentes.", obj({ query: str }, ["query"])),
  tool("agenda.create_event", "Crée un événement : { text } en français (« demain 14h dentiste ») ou { title, start_ms, end_ms?, all_day? }.", obj({ text: str, title: str, start_ms: { type: "integer" }, end_ms: { type: "integer" }, all_day: { type: "boolean" } })),
  tool("music.play_playlist", "Lance une de mes playlists Spotify, par identifiant ou par nom.", obj({ id: str, name: str })),
  tool("music.play_search", "Cherche sur Spotify et lance le premier résultat (titre, artiste ou playlist).", obj({ query: str, kind: { type: "string", enum: ["track", "artist", "playlist"] } }, ["query"])),
  tool("music.volume", "Règle le volume de Spotify : `percent` (0 à 100) ou `delta` (relatif).", obj({ percent: { type: "integer" }, delta: { type: "integer" } })),
  tool("mail.list", "Mails récents de la boîte de réception (cache local) : expéditeur, sujet, extrait, tri.", obj({ bucket: { enum: ["todo", "read", "filtered"] }, account: { type: "integer" }, limit: { type: "integer" } })),
  tool("focus.start", "Lance une session de focus (durée des réglages), suivie d'une pause.", obj({})),
  tool("iris.ask_claude", "Confie la demande à Claude, plus capable, quand elle te dépasse : { task }.", obj({ task: str }, ["task"])),
];

// expect : outils attendus (dans l'ordre) ; null = réponse en texte attendue, sans outil.
const CASES = [
  { q: "Mets ma playlist Focus", expect: ["music__play_playlist"] },
  { q: "Rappelle-moi d'appeler le garage jeudi à 14 h", expect: ["reminders__create"] },
  { q: "Baisse un peu le son de Spotify", expect: ["music__volume"] },
  { q: "J'ai des mails non lus de Paul ?", expect: ["mail__list"] },
  { q: "Note ma liste de courses : pâtes, œufs, café. Et rappelle-moi d'y aller samedi à 10 h.", expect: ["notes__create", "reminders__create"] },
  { q: "Ajoute dentiste mardi prochain à 9 h 30 à mon agenda", expect: ["agenda__create_event"] },
  { q: "Combien font 17 % de 2 340 € ?", expect: null, check: (t) => /397[,.]8/.test(t) },
  { q: "En trois points courts : pourquoi SQLite plutôt que PostgreSQL pour une app de bureau ?", expect: null },
  { q: "En Rust, j'ai « borrowed value does not live long enough » dans une closure passée à tokio::spawn. Cause probable et correction, en bref.", expect: null, check: (t) => /move|'static|Arc|clone/i.test(t) },
  { q: "Écris un court mail poli pour décaler la réunion de demain à vendredi même heure.", expect: null },
];

async function chat(model, content) {
  const body = {
    model, stream: false, think: false, keep_alive: "5m",
    options: { temperature: 0.2, num_ctx: 16384 },
    messages: [{ role: "system", content: SYSTEM }, { role: "user", content }],
    tools: TOOLS,
  };
  const t0 = performance.now();
  const r = await fetch(`${OLLAMA}/api/chat`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(`${model} : HTTP ${r.status} ${await r.text()}`);
  const j = await r.json();
  return {
    ms: Math.round(performance.now() - t0),
    tokS: j.eval_duration ? j.eval_count / (j.eval_duration / 1e9) : 0,
    text: (j.message?.content ?? "").trim(),
    calls: (j.message?.tool_calls ?? []).map((c) => ({ name: c.function.name, args: c.function.arguments })),
  };
}

function verdict(c, r) {
  if (c.expect) {
    const got = r.calls.map((x) => x.name);
    return c.expect.every((e) => got.includes(e)) && got.length === c.expect.length ? "✓" : "✗";
  }
  if (r.calls.length) return "✗";
  return c.check ? (c.check(r.text) ? "✓" : "✗") : "·";
}

async function gpuShare(model) {
  const ps = await fetch(`${OLLAMA}/api/ps`).then((r) => r.json()).catch(() => ({ models: [] }));
  const m = ps.models?.find((x) => x.name === model || x.model === model);
  return m && m.size ? Math.round((100 * (m.size_vram ?? 0)) / m.size) : null;
}

const results = {};
for (const model of MODELS) {
  process.stdout.write(`\n== ${model} : chargement… `);
  const warm = await chat(model, "Bonjour").catch((e) => { console.log(`\n${e.message}\nTélécharge-le : ollama pull ${model}`); process.exit(1); });
  const gpu = await gpuShare(model);
  console.log(`${(warm.ms / 1000).toFixed(1)} s (chargement compris), ${gpu === null ? "GPU ?" : `${gpu} % sur le GPU`}`);
  results[model] = { gpu, load: warm.ms, rows: [] };
  for (const c of CASES) {
    const r = await chat(model, c.q);
    const v = verdict(c, r);
    results[model].rows.push({ ...r, v });
    console.log(`  ${v} ${(r.ms / 1000).toFixed(1).padStart(5)} s ${r.tokS.toFixed(0).padStart(3)} tok/s  ${c.q.slice(0, 60)}`);
  }
  // Libère la mémoire vidéo avant le modèle suivant.
  await fetch(`${OLLAMA}/api/generate`, { method: "POST", body: JSON.stringify({ model, keep_alive: 0 }) }).catch(() => {});
}

const avg = (xs) => xs.reduce((a, b) => a + b, 0) / Math.max(1, xs.length);
console.log("\n== Bilan");
console.log("modèle            | réussis | temps moyen | tok/s | GPU");
for (const m of MODELS) {
  const rows = results[m].rows;
  const ok = rows.filter((r) => r.v === "✓").length, scored = rows.filter((r) => r.v !== "·").length;
  console.log(`${m.padEnd(17)} |  ${ok}/${scored}   |   ${(avg(rows.map((r) => r.ms)) / 1000).toFixed(1).padStart(5)} s   | ${avg(rows.map((r) => r.tokS)).toFixed(0).padStart(4)}  | ${results[m].gpu ?? "?"} %`);
}

let md = `# Comparaison des modèles locaux — ${now.toLocaleString("fr-FR")}\n\nConditions d'IRIS : think désactivé, température 0,2, contexte 16 384, ${TOOLS.length} outils. ✓ bon outil ou bonne réponse, ✗ raté, · à juger toi-même.\n`;
CASES.forEach((c, i) => {
  md += `\n## ${i + 1}. ${c.q}\n`;
  for (const m of MODELS) {
    const r = results[m].rows[i];
    md += `\n**${m}** — ${r.v} — ${(r.ms / 1000).toFixed(1)} s, ${r.tokS.toFixed(0)} tok/s\n\n`;
    if (r.calls.length) md += r.calls.map((x) => `- outil \`${x.name}\` ${JSON.stringify(x.args)}`).join("\n") + "\n";
    if (r.text) md += `\n> ${r.text.replace(/\n/g, "\n> ")}\n`;
  }
});
const out = join(dirname(fileURLToPath(import.meta.url)), `results-${now.toISOString().slice(0, 16).replace(/[:T]/g, "-")}.md`);
writeFileSync(out, md);
console.log(`\nRéponses complètes, à lire côte à côte : ${out}`);
