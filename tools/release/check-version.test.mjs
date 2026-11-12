import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./check-version.mjs', import.meta.url));

function repo(v, core = v) {
  const d = mkdtempSync(join(tmpdir(), 'cv-'));
  mkdirSync(join(d, 'src-tauri'));
  mkdirSync(join(d, 'crates/core'), { recursive: true });
  writeFileSync(join(d, 'package.json'), JSON.stringify({ version: v }));
  writeFileSync(join(d, 'src-tauri/tauri.conf.json'), JSON.stringify({ version: v }));
  writeFileSync(join(d, 'src-tauri/Cargo.toml'), `[package]\nname = "oniris"\nversion = "${v}"\n`);
  writeFileSync(join(d, 'crates/core/Cargo.toml'), `[package]\nname = "oniris-core"\nversion = "${core}"\n`);
  return d;
}

const run = (cwd, ...args) => spawnSync(process.execPath, [script, ...args], { cwd, encoding: 'utf8' });

test('tout cohérent avec la bonne étiquette : code 0', () => {
  assert.equal(run(repo('1.2.0'), 'v1.2.0').status, 0);
});

test('Cargo.toml du cœur en retard : code 1 et fichier nommé', () => {
  const r = run(repo('1.2.0', '1.1.0'), 'v1.2.0');
  assert.equal(r.status, 1);
  assert.match(r.stderr, /crates\/core\/Cargo\.toml/);
});

test('étiquette différente de la version : code 1', () => {
  assert.equal(run(repo('1.2.0'), 'v1.2.1').status, 1);
});

test('sans étiquette, seuls les fichiers sont comparés', () => {
  assert.equal(run(repo('1.2.0')).status, 0);
});
