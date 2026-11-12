# Oniris — Plan 9 : Versions publiées et mise à jour automatique

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :**
- L'utilisateur valide une version ; GitHub la fabrique, la signe et la publie.
- Oniris la télécharge en arrière-plan et l'installe sans geste manuel, sans jamais déranger.

**Architecture :**
- **Publication :** une étiquette `vX.Y.Z` poussée déclenche un workflow GitHub Actions Windows (`tauri-apps/tauri-action`). Il vérifie la version, lance les tests, fabrique le NSIS, le signe pour la mise à jour, puis publie la release avec `latest.json`.
- **Côté app :**
  - un module Rust `updater` interroge `latest.json` et télécharge l'installateur via `tauri-plugin-updater` (signature vérifiée) ;
  - il garde l'installateur sur le disque ;
  - il le lance lui-même, au clic « Redémarrer » (avec relance) ou à la fermeture (sans relance), seulement quand l'utilisateur est au calme.
- **Logique pure :** les décisions (quand vérifier, quand installer) vivent dans `oniris-core` et sont testées sans réseau.

**Tech Stack :** celle des plans 1 à 8, plus :
- `tauri-plugin-updater` 2.x (MIT/Apache-2.0) ;
- GitHub Actions `windows-latest` avec `tauri-apps/tauri-action`, `pnpm/action-setup`, `actions/setup-node`, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`.

**Spec :** [`docs/superpowers/specs/2026-11-12-oniris-mises-a-jour-design.md`](../specs/2026-11-12-oniris-mises-a-jour-design.md), dont la section « Précisions ».

## Global Constraints

- **Dépôt public :** `NathanChevrollier/Oniris`.
  - Point de mise à jour : `https://github.com/NathanChevrollier/Oniris/releases/latest/download/latest.json`.
- **Version :**
  - identique dans `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` et `crates/core/Cargo.toml` ;
  - l'étiquette vaut `v` suivi de cette version.
  - Première version avec mise à jour : **1.2.0**.
- **Secrets :**
  - la clé privée de signature et son mot de passe ne sont **jamais** dans le dépôt, un journal ou une sortie de commande ;
  - ils vivent seulement dans les secrets GitHub `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, et dans le 1Password de l'utilisateur ;
  - la clé publique va dans `tauri.conf.json` > `plugins.updater.pubkey`.
- **Vérification :** 3 min après le démarrage, puis toutes les 6 h. Jamais quand le réglage `auto_update` est faux (défaut : vrai). « Vérifier maintenant » marche toujours.
- **Installation, seulement au calme,** c'est-à-dire quand aucune de ces conditions n'est vraie :
  - le pipeline voix est occupé (tour en cours, écoute, transcription ou IRIS qui parle) ;
  - `fullscreen::active()` est vrai.
- **Bouton « Redémarrer » :** installation passive, puis relance. **Fermeture :** installation passive sans relance. Rien ne s'exécute pendant l'extinction de Windows.
- **Échec** (réseau, signature, installation) : une ligne `warn` dans le journal, aucun toast, nouvel essai au cycle suivant.
- **Données conservées :** l'installateur NSIS par utilisateur (plan 7) ne touche ni `%LOCALAPPDATA%\Oniris` ni `%USERPROFILE%\oniris-wakeword`.
- **Textes :** interface en français, avec tutoiement. Licence « Tous droits réservés » en français et en anglais. Champs `license` = `LicenseRef-Proprietary`.
- **Règles R8 pour les sous-agents :**
  - jamais `tauri dev` ni `tauri build` ;
  - jamais de push ni de `gh secret` ;
  - pas de réseau dans les tests ;
  - aucune fenêtre ni frappe simulée sur le bureau.

  Le contrôleur fait lui-même les étapes Git et GitHub (Tâches 0, 5 et 6), avec l'accord de l'utilisateur pour tout push.
- **Commits :** Conventional Commits en français, signés via 1Password, sans co-signature Claude.

## Review Focus

1. **L'utilisateur ferme Oniris pendant que la mise à jour se télécharge.** L'installateur partiel n'est jamais lancé : seul un fichier complet et vérifié est installable. Test en Tâche 2 (`partial_download_is_never_installed`).
2. **Deux versions sortent avant que la première soit installée.** C'est la plus récente qui s'installe, et l'ancien fichier téléchargé est supprimé. Test en Tâche 2 (`newer_download_replaces_pending`).
3. **Le PC est hors ligne, ou GitHub répond 404** (aucune release, juste après l'ouverture du dépôt). Pas de toast, pas de boucle serrée, nouvel essai à 6 h. Test en Tâche 1 (`failure_waits_full_interval`).
4. **La version distante est égale ou plus ancienne** (republication, retour arrière). Rien n'est téléchargé. Test en Tâche 1 (`only_strictly_newer_versions`).
5. **Clic sur « Redémarrer » pendant qu'IRIS parle.** L'installation attend la fin, puis part toute seule. Elle n'est jamais perdue. Test en Tâche 2 (`restart_waits_for_calm`).

---

### Tâche 0 : Clé de signature et préparation de GitHub (contrôleur, pas de sous-agent)

- [ ] Générer la paire de clés dans un dossier temporaire hors du dépôt : `pnpm tauri signer generate -w <tmp>/oniris-updater.key -p <mot de passe aléatoire de 32 caractères>`.
  - Ne jamais afficher la clé privée.
  - Garder la clé publique pour la Tâche 2.
- [ ] Déposer les secrets sur GitHub sans les afficher :
  - `gh secret set TAURI_SIGNING_PRIVATE_KEY -R NathanChevrollier/Oniris < <tmp>/oniris-updater.key` ;
  - le mot de passe dans `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, par l'entrée standard.
- [ ] Régler les permissions Actions du dépôt : lecture et écriture, nécessaires à la création de releases (`gh api -X PUT repos/NathanChevrollier/Oniris/actions/permissions/workflow -f default_workflow_permissions=write`).
- [ ] Demander à l'utilisateur de ranger dans 1Password le fichier de clé privée et le mot de passe.
  - Lui donner le chemin du fichier et le mot de passe en privé, dans le terminal.
  - Supprimer le fichier local **après** sa confirmation.
- [ ] Journal du plan : clé publique notée, et date de la sauvegarde confirmée.

### Tâche 1 : Préparer le dépôt et la logique de décision (`oniris-core`)

**Files:**
- Create : `crates/core/src/update.rs`, `LICENSE`, `NOTICE.md`
- Modify :
  - `crates/core/src/lib.rs` (`pub mod update;`) ;
  - `crates/core/src/settings.rs` (`auto_update: bool`, défaut `true`) ;
  - `src/lib/settings.ts` (type et défaut) ;
  - `src-tauri/tests/nsis_config.rs` (version lue dans `tauri.conf.json`, plus de `1.0.0` en dur : le test échoue depuis le passage en 1.1.0) ;
  - `src-tauri/src/modules/search.rs` (test instable `indexation_vraie_jusqu_a_la_fin_du_premier_parcours`, course entre `indexing()` et `IndexStatus::Ready`) ;
  - `package.json`, `src-tauri/Cargo.toml`, `crates/core/Cargo.toml` (`license = "LicenseRef-Proprietary"`) ;
  - `README.md` (section « Licence ») ;
  - `.gitignore` (vérifier `target/`, `node_modules/`, `.superpowers/`, `graphify-out/`, `*.key`, `.env*`).
- Test : `crates/core/src/update.rs` (module `tests`), `src-tauri/tests/nsis_config.rs`, `src-tauri/src/modules/search.rs`

**Interfaces:**
- Produces (dans `oniris_core::update`) :
  - `pub const FIRST_CHECK: Duration = 180 s;` et `pub const INTERVAL: Duration = 6 h;`
  - `pub fn is_newer(current: &str, remote: &str) -> bool` : comparaison semver stricte ; `false` si une version est illisible.
  - `pub struct Schedule { next: Instant }`
    - `pub fn new(start: Instant) -> Self` : premier essai à `start + FIRST_CHECK` ;
    - `pub fn due(&self, now: Instant, enabled: bool) -> bool` ;
    - `pub fn done(&mut self, now: Instant)` : prochain essai à `now + INTERVAL`, que l'essai ait réussi ou non.
  - `pub struct Calm { pub voice_busy: bool, pub fullscreen: bool }` et `pub fn can_install(c: Calm) -> bool`.
  - `Settings.auto_update: bool` (défaut `true`).

- [ ] **Step 1 : écrire les tests qui échouent**

```rust
// update.rs
#[test] fn only_strictly_newer_versions() { /* is_newer("1.2.0","1.2.1") ; !is_newer("1.2.0","1.2.0") ; !is_newer("1.2.0","1.1.9") ; is_newer("1.9.0","1.10.0") ; !is_newer("1.2.0","pas-une-version") */ }
#[test] fn first_check_after_three_minutes() { /* due(start+179 s) faux ; due(start+180 s) vrai */ }
#[test] fn failure_waits_full_interval() { /* done(t) puis due(t+5 h 59) faux, due(t+6 h) vrai */ }
#[test] fn disabled_never_due() { /* enabled=false → due toujours faux */ }
#[test] fn install_only_when_calm() { /* can_install vrai seulement si voice_busy=false et fullscreen=false */ }
// settings.rs
#[test] fn auto_update_defaults_to_true() { assert!(Settings::default().auto_update); }
// nsis_config.rs : les tests lisent la version depuis tauri.conf.json et vérifient qu'elle est identique dans les 4 fichiers
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris-core update`. Attendu : ÉCHEC à la compilation.
- [ ] **Step 3 : écrire le code**
  - `update.rs` : l'utilisation du crate `semver` est permise (vérifier d'abord s'il est déjà au lockfile).
  - **Test de recherche :** corriger la cause (attendre un signal de fin d'indexation, ou relever `IndexStatus` après la fin), pas en augmentant une attente.
  - **`LICENSE` :**
    - « Tous droits réservés © 2026 Nathan Chevrollier » ;
    - la consultation du code est permise ;
    - la copie, la modification, la redistribution et l'usage commercial sont interdits sans accord écrit ;
    - le tout en français puis en anglais.
  - **`NOTICE.md` :**
    - les composants tiers et leurs licences, lues dans `Cargo.lock`, `package.json` et les kits : openWakeWord (Apache-2.0), tract, whisper.cpp, voix Piper (licence de chaque voix utilisée), Tauri ;
    - une section « Modèle du mot d'activation » : `dis_iris.onnx` dérive de caractéristiques ACAV100M et de modèles openWakeWord publiés pour un usage non commercial. Citer la licence exacte depuis le README du dépôt openWakeWord. Il est fourni pour un usage personnel non commercial.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.**
  - `cargo test -p oniris-core`, puis `cargo test -p oniris`.
  - `cargo test -p oniris search::` 10 fois de suite, sans un seul échec.
  - `pnpm vitest run`.
- [ ] **Step 5 : commit.** `chore(depot): licence, notices et tests prêts pour la CI`

### Tâche 2 : Module de mise à jour dans l'app (Rust)

**Files:**
- Create : `src-tauri/src/modules/updater.rs`
- Modify :
  - `src-tauri/src/modules/mod.rs` ;
  - `src-tauri/Cargo.toml` (`tauri-plugin-updater = "2"`) ;
  - `src-tauri/src/lib.rs` (plugin, démarrage du module, `RunEvent::Exit`) ;
  - `src-tauri/tauri.conf.json` (`bundle.createUpdaterArtifacts: true` ; `plugins.updater.endpoints` = [le point de mise à jour] ; `plugins.updater.pubkey` = la clé publique de la Tâche 0 ; `plugins.updater.windows.installMode: "passive"`) ;
  - `src-tauri/src/modules/voice/pipeline.rs` (`pub fn voice_busy() -> bool` : tour en cours, phase différente d'Idle, ou IRIS qui parle).
- Test : `src-tauri/src/modules/updater.rs` (module `tests`, avec une fausse source de mises à jour)

**Interfaces:**
- Consumes : `oniris_core::update::{is_newer, Schedule, Calm, can_install}` et `Settings.auto_update` (Tâche 1) ; `fullscreen::active()`.
- Produces :
  - `pub trait Source: Send + Sync { async fn latest(&self) -> Result<Option<Remote>, String>; async fn download(&self, r: &Remote, to: &Path) -> Result<(), String>; }`
    - Si le trait doit servir d'objet dynamique, utiliser `async_trait`. Implémenté par `GithubSource`, qui enveloppe `tauri_plugin_updater::UpdaterExt` : `check()`, puis `download()`, qui vérifie la signature. Les octets sont écrits dans un `.part`, puis renommés en `.exe` seulement après vérification.
    - Une `FakeSource` sert aux tests.
  - `pub struct Remote { pub version: String, pub notes: String }`
  - `pub struct Pending { pub version: String, pub notes: String, pub installer: PathBuf }`, dans `%LOCALAPPDATA%\Oniris\updates\`.
  - `pub fn start(app: &AppHandle)` : une boucle qui suit `Schedule`.
  - Commandes Tauri :
    - `update_status() -> UpdateStatus { current, pending: Option<(version, notes)>, checking: bool, last_error: Option<String> }` ;
    - `update_check_now()`, en `async` ;
    - `update_restart()`.
  - Événement `oniris://update` avec `{ version, notes }`, émis quand une mise à jour est prête.
  - `pub fn install_on_exit(app: &AppHandle)`, appelé dans `RunEvent::Exit` : si une mise à jour est prête **et** que c'est le calme, il lance l'installateur en passif **sans** relance, détaché, puis rend la main.
  - `update_restart` : attend le calme (relu toutes les 2 s), puis lance l'installateur en passif **avec** relance et quitte l'app.
  - **Arguments de ligne de commande :** reprendre exactement ceux de `tauri-plugin-updater` pour NSIS en mode passif (lire sa source pour Windows dans le registre cargo), avec et sans relance. Les noter en commentaire.

- [ ] **Step 1 : écrire les tests qui échouent**

```rust
#[tokio::test] async fn downloads_only_newer_and_emits_once()     { /* FakeSource 1.2.1 avec current 1.2.0 → Pending 1.2.1, un seul événement ; refaire un cycle → aucun nouveau téléchargement */ }
#[tokio::test] async fn partial_download_is_never_installed()     { /* download échoue en cours : aucun .exe dans updates/, pending reste None, le .part est supprimé */ }
#[tokio::test] async fn newer_download_replaces_pending()         { /* pending 1.2.1, puis la source annonce 1.2.2 → pending 1.2.2, l'installateur 1.2.1 est supprimé */ }
#[tokio::test] async fn restart_waits_for_calm()                  { /* voice_busy vrai : update_restart ne lance rien ; voice_busy faux 3 s plus tard → lanceur appelé une fois avec relance */ }
#[tokio::test] async fn exit_installs_without_relaunch_only_when_calm() { /* fullscreen vrai → rien ; calme → lanceur appelé sans relance */ }
#[tokio::test] async fn disabled_setting_never_contacts_source()  { /* auto_update=false : latest() jamais appelé en 7 h simulées ; update_check_now l'appelle une fois */ }
#[tokio::test] async fn source_error_is_quiet()                   { /* latest() → Err : aucun toast, last_error renseigné, prochain essai à +6 h */ }
```

  Le lanceur d'installateur est injecté sous la forme d'une fonction `Fn(&Path, relaunch: bool)` : les tests n'exécutent rien.
- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris updater`. Attendu : ÉCHEC à la compilation.
- [ ] **Step 3 : écrire le module et le brancher**, avec l'horloge tokio, en pause dans les tests.
  - Au démarrage, supprimer de `updates\` tout installateur dont la version est inférieure ou égale à la version courante : cela veut dire que la mise à jour est faite.
  - Aucune requête réseau dans les tests.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** `cargo test -p oniris`. Attendu : PASS, sans nouveau warning.
- [ ] **Step 5 : commit.** `feat(maj): téléchargement et installation des mises à jour au calme`

### Tâche 3 : Interface de mise à jour, et confidentialité

**Files:**
- Modify :
  - `src/pill/Pill.tsx`, `src/pill/pill.css`, `src/pill/Pill.test.tsx` ;
  - `src/modules/settings/SettingsSheet.tsx`, `src/modules/settings/settings.test.tsx` ;
  - un petit client `src/modules/settings/update.ts` ;
  - `docs/legal/confidentialite.md` (v10) ;
  - `docs/verifications/plan-09-mises-a-jour.md` (à créer).

**Interfaces:**
- Consumes : les commandes `update_status`, `update_check_now` et `update_restart`, et l'événement `oniris://update` (Tâche 2).

- [ ] **Step 1 : écrire les tests qui échouent**

```tsx
it("la pilule annonce une mise à jour prête", ...)        // événement {version:"1.2.1"} → texte « Mise à jour 1.2.1 prête » + bouton « Redémarrer » ; clic → invoke("update_restart")
it("l'annonce reste discrète et se ferme", ...)            // bouton « Plus tard » → l'annonce disparaît, l'installation se fera à la fermeture
it("réglages : mises à jour automatiques cochées par défaut", ...) // libellé « Mises à jour automatiques » ; décocher + Enregistrer → save({auto_update:false})
it("réglages : version installée et vérification manuelle", ...)   // « Version 1.2.0 » ; clic « Vérifier maintenant » → invoke("update_check_now") ; si pending : « Version 1.2.1 prête » et ses notes affichées
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `pnpm vitest run src/pill src/modules/settings`. Attendu : ÉCHEC.
- [ ] **Step 3 : écrire le code**
  - **Annonce de la pilule :** dans le style des toasts existants, sans voler le focus ni ouvrir de fenêtre.
  - **Note de réglage exacte :** « Oniris vérifie toutes les 6 h s'il existe une nouvelle version sur GitHub, la télécharge en arrière-plan et l'installe quand tu fermes l'app, jamais pendant que tu joues ou que tu parles à IRIS. »
  - **Confidentialité v10,** nouvelle section « Mises à jour » :
    - ce qui est contacté (`github.com`, qui voit ton adresse IP et la version d'Oniris dans la requête) ;
    - quand c'est contacté ;
    - la vérification de signature ;
    - comment couper.
  - **Liste de vérification :**
    - installer la 1.2.0 à la main ;
    - publier une 1.2.1 de test ;
    - observer l'annonce, puis « Redémarrer » ;
    - observer l'installation à la fermeture ;
    - vérifier que réglages, base, voix et extraits d'apprentissage sont conservés ;
    - couper le réseau : aucun toast.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** `pnpm vitest run`, `pnpm lint` et `pnpm build`. Attendu : tout est vert.
- [ ] **Step 5 : commit.** `feat(maj): annonce dans la pilule et réglage des mises à jour`

### Tâche 4 : Workflow de publication et documentation

**Files:**
- Create :
  - `.github/workflows/release.yml` ;
  - `tools/release/check-version.mjs`, avec son test `tools/release/check-version.test.mjs`, lancé par `node --test`.
- Modify : `README.md` (section « Sortir une version »), `package.json` (script `"check:version": "node tools/release/check-version.mjs"`).

**Interfaces:**
- Produces :
  - `node tools/release/check-version.mjs [vX.Y.Z]` : vérifie que les 4 fichiers portent la même version et que l'étiquette, si elle est donnée, vaut `v` + cette version.
  - Sortie : code 0 si tout est cohérent. Sinon, code 1 et un message en français qui nomme le fichier fautif.

- [ ] **Step 1 : écrire les tests qui échouent** (`check-version.test.mjs`, sur un dossier temporaire)
  - 4 fichiers en 1.2.0 avec `v1.2.0` → 0 ;
  - `Cargo.toml` du cœur en 1.1.0 → 1, et le message nomme `crates/core/Cargo.toml` ;
  - `v1.2.1` alors que les fichiers sont en 1.2.0 → 1.
- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `node --test tools/release/`. Attendu : ÉCHEC.
- [ ] **Step 3 : écrire le script et le workflow**
  - Le workflow se déclenche sur `push: tags: ['v*']` et tourne sur `windows-latest`, avec `permissions: contents: write`. Ses étapes :
    1. checkout ;
    2. pnpm et Node LTS, avec cache ;
    3. Rust stable, avec rust-cache ;
    4. `pnpm install --frozen-lockfile` ;
    5. `node tools/release/check-version.mjs ${{ github.ref_name }}` ;
    6. `cargo test -p oniris-core`, `cargo test -p oniris` et `pnpm vitest run` ;
    7. `tauri-apps/tauri-action`, avec ses paramètres :
       - `tagName: ${{ github.ref_name }}` ;
       - `releaseName: "Oniris ${{ github.ref_name }}"` ;
       - `releaseBody` : le contenu de `docs/releases/${{ github.ref_name }}.md`, lu dans une étape précédente ;
       - `releaseDraft: false` ;
       - `includeUpdaterJson: true` ;
       - l'environnement `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, issus des secrets.
  - Les notes de chaque version vivent dans `docs/releases/vX.Y.Z.md`. Le workflow échoue si ce fichier manque.
  - Le README « Sortir une version » explique en 5 lignes ce que fait le contrôleur quand l'utilisateur dit « sors la version X.Y.Z » :
    1. passer la version dans les 4 fichiers ;
    2. écrire les notes ;
    3. lancer `check:version` ;
    4. commiter et étiqueter ;
    5. montrer le tout, puis pousser après accord.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.**
  - `node --test tools/release/` ;
  - `node tools/release/check-version.mjs`, sur le dépôt, en 1.1.0 → 0 ;
  - `actionlint` s'il est disponible, sinon relecture.
- [ ] **Step 5 : commit.** `ci(release): publication signée sur étiquette`

### Tâche 5 : Audit avant ouverture (contrôleur, pas de sous-agent)

- [ ] Analyser **tout l'historique** avec gitleaks (binaire téléchargé dans le dossier temporaire, rien n'est ajouté au projet), puis le compléter par des recherches ciblées dans `git log -p` :
  - `client_secret`, `refresh_token`, `api_key`, `password`, `BEGIN .* PRIVATE KEY`, `ghp_`, `sk-` ;
  - adresses IP ou nom de domaine du VPS ;
  - adresses e-mail.
- [ ] Vérifier qu'aucun fichier lourd ou privé n'est suivi : enregistrements de voix, bases SQLite, `.env`.
- [ ] **Montrer le rapport à l'utilisateur.**
  - Un secret trouvé est révoqué dans tous les cas, et l'utilisateur choisit s'il faut réécrire l'historique.
  - Les données personnelles sont seulement signalées.
- [ ] **Aucun push avant son accord explicite.**

### Tâche 6 : Première publication, 1.2.0 (contrôleur, avec l'utilisateur)

- [ ] Après la revue finale du plan et la fusion :
  - passer la version à 1.2.0 ;
  - écrire `docs/releases/v1.2.0.md` : réveil vocal, mode apprentissage, mises à jour automatiques, boutons de l'en-tête ;
  - lancer `check:version` ;
  - commit `chore(release): version 1.2.0`, puis l'étiquette `v1.2.0`.
- [ ] Montrer à l'utilisateur ce qui part. Après son accord :
  - `git remote add origin https://github.com/NathanChevrollier/Oniris.git` ;
  - `git push -u origin main` ;
  - `git push origin v1.2.0`.
- [ ] Suivre le workflow (`gh run watch`). En cas d'échec, corriger, puis publier une nouvelle étiquette **après accord**. Ne jamais forcer un push.
- [ ] Vérifier la release publiée : l'installateur, sa signature `.sig`, et un `latest.json` qui pointe vers la 1.2.0.
- [ ] L'utilisateur installe la 1.2.0 à la main, une seule fois.
- [ ] La vraie mise à jour automatique sera vérifiée avec la prochaine version qu'il validera, en suivant `docs/verifications/plan-09-mises-a-jour.md`.

---

## Après le plan

- Revue finale de la branche, puis fusion `--ff-only` et `graphify update .`.
- **Feuille de route :** ajouter la ligne 9.
- **Spec principale :** mention de la publication automatique.
