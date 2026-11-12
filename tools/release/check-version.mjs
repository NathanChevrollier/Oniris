// Vérifie que les 4 fichiers portent la même version et que l'étiquette (optionnelle) vaut `v` + cette version.
import { readFileSync } from 'node:fs';

const lit = (f) => readFileSync(f, 'utf8');
const json = (f) => JSON.parse(lit(f)).version;
// shortcut: premier `version = "…"` du fichier = celui de [package] ; vrai tant que [package] reste en tête.
const toml = (f) => /^version\s*=\s*"([^"]+)"/m.exec(lit(f))?.[1];

const fichiers = [
  ['package.json', json],
  ['src-tauri/tauri.conf.json', json],
  ['src-tauri/Cargo.toml', toml],
  ['crates/core/Cargo.toml', toml],
];

const versions = fichiers.map(([f, lire]) => {
  try {
    return [f, lire(f)];
  } catch (e) {
    return [f, undefined];
  }
});
const attendue = versions[0][1];
const erreurs = versions
  .filter(([, v]) => !v || v !== attendue)
  .map(([f, v]) => `${f} : version ${v ?? 'illisible'} au lieu de ${attendue ?? '?'} (référence : package.json)`);

const etiquette = process.argv[2];
if (etiquette && etiquette !== `v${attendue}`) {
  erreurs.push(`étiquette ${etiquette} au lieu de v${attendue} (version de package.json)`);
}

if (erreurs.length) {
  console.error(`Versions incohérentes :\n- ${erreurs.join('\n- ')}`);
  process.exit(1);
}
console.log(`Versions cohérentes : ${attendue}${etiquette ? ` (${etiquette})` : ''}`);
