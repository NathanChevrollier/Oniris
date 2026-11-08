# Oniris — Plan 7 : Finitions

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Rendre Oniris prêt à être installé et utilisé chaque jour :
- un écran **Paramètres** complet ;
- un **premier lancement** guidé, avec consentement au démarrage automatique ;
- un démarrage et un arrêt robustes ;
- les reports utiles des plans 1 à 6 ;
- une passe de **performance** sur ce qui se mesure sans l'app réelle ;
- des **outils de mesure** prêts pour l'utilisateur (budgets du §9, fuite sur 24 h) ;
- la **configuration d'empaquetage NSIS**.

Le build release n'est **pas** lancé : il attend l'accord de l'utilisateur.

**Architecture :** Rien de nouveau. Les écrans suivent le modèle des feuilles existantes (`AccountsSheet`, `IrisSheet` : `inert` derrière, focus rendu au bouton de l'en-tête, `useSave` qui relit les réglages avant d'écrire). Les corrections vont dans les modules qu'elles touchent.

**Tech Stack :** Celle des plans 1 à 6, plus `eslint` avec `eslint-plugin-react-hooks` en dépendance de développement. Aucune autre dépendance.

**Spec :** [`docs/superpowers/specs/2026-11-03-oniris-design.md`](../specs/2026-11-03-oniris-design.md), §4.4, §8, §9, §10, §11 et §12 (« écrans de paramètres et du premier lancement »).

**Journaux précédents :** 1 à 6, dans `docs/superpowers/plans/*-journal.md`. Les reports retenus sont repris ci-dessous ; les autres restent notés dans ces journaux.

## Décisions prises avant ce plan (2026-11-08)

- **Mesures sur l'app réelle.** Ouverture < 100 ms, RAM, CPU au repos, VRAM, fuite sur 24 h, latences vocales : **l'utilisateur les lance**. Les sous-agents ne peuvent ni lancer l'app ni la laisser tourner (règle R8).
  - Ce plan livre les outils et la procédure (un script par mesure, un verdict lisible), plus une liste de vérification finale.
  - Si une mesure réelle dépasse son budget, la correction fera l'objet d'un petit plan 7b. C'est aussi le cas de Whisper sur GPU si la latence dépasse 1 s (P6-R4).
- **Démarrage automatique** (règle R15 du plan 1). Le défaut reste `true`, mais **rien n'est écrit dans la clé `Run` avant le consentement donné au premier lancement**. Le premier lancement le demande explicitement.
- **Version 1.0.0.** Installation NSIS **par utilisateur** (`currentUser`, sans droits administrateur), en français.
  - L'installeur ne télécharge rien : Ollama, les modèles et la voix restent des installations à la demande.
  - Pas de mise à jour automatique.
- **Reports retenus** (les autres restent dans les journaux) :
  - Plan 1 : erreur de démarrage invisible (dialogue et journal), checkpoint WAL à la fermeture, réglages chargés re-validés (`hover_delay_ms`).
  - Plan 2 : bascule du presse-papiers depuis la zone de notification jamais resynchronisée avec les autres enregistrements.
  - Plan 3 :
    - `.pal-pop` toujours monté, avec un `backdrop-filter` à opacité 0 ;
    - table `usage` jamais élaguée ;
    - `notes::search` qui verrouille la base à chaque frappe ;
    - pas de lint `react-hooks` ;
    - pas d'interface pour ré-afficher un projet masqué.
  - Plan 4 :
    - enfant Zenytt orphelin : passer par `job::spawn` ;
    - cache des playlists Spotify gardé après la suppression du compte.
  - Plan 5 :
    - perte du focus à l'arrivée d'une confirmation ;
    - tour abandonné qui saute `unload_if_fullscreen`.
  - Plan 6 :
    - `installed()` des archives ne vérifie que le dossier (antivirus, suppression manuelle) ;
    - `speak` qui bloque un fil tokio ;
    - pilule entièrement re-rendue à chaque niveau de voix ;
    - `clean()` qui écarte « active le sous-titrage ».

## Global Constraints

- **Héritées des plans 2 à 6 :**
  - une seule liste `CORE_MIGRATIONS` ;
  - commandes déclarées dans `build.rs`, `capabilities/panel.json` et `generate_handler!` ;
  - commandes qui touchent la base en `async` ;
  - chemin d'ouverture du panneau sans verrou de base ;
  - journal technique sans contenu utilisateur ;
  - processus sans console, dans un Job Object ;
  - commits signés, sans co-signature.
- **Interface :** français, tutoiement, le style des feuilles existantes, accessible au clavier (focus visible, `aria-*`).
  - Le panneau tient en **2560×1440 et 1920×1080 sans le fondu `.more`**, mesuré par CDP. La mesure 1080p est nouvelle.
- **R8 :**
  - pas de `tauri dev` ni de `tauri build`, aucune fenêtre ouverte ;
  - pas de vrai micro, haut-parleur, Ollama, Claude, Whisper ni Piper ;
  - pas d'Internet ;
  - **aucune écriture dans la clé `Run` du registre** : l'autostart passe derrière un trait ;
  - Vite sur un port autre que 1420, arrêté par `taskkill /T`.

## Review Focus

1. **Premier lancement interrompu** (fermé au milieu, PC éteint). Au lancement suivant, l'écran revient. Rien n'a été écrit dans `Run` sans clic sur « Oui ». Les choix déjà faits sont gardés. Tests dans la tâche 3.
2. **Réglage invalide saisi dans Paramètres** (heures inversées, dossier inexistant, raccourci déjà pris par Windows). Le message de Rust s'affiche à côté du champ, rien n'est enregistré à moitié, et les autres sections ne perdent pas leur saisie. Tests dans la tâche 2.
3. **Base corrompue, verrouillée par un autre processus, ou `%APPDATA%` en lecture seule au démarrage.** Un dialogue en français explique, avec le chemin du journal. Aucune fermeture silencieuse. Tests dans la tâche 1.
4. **Écran 1920×1080 avec mise à l'échelle 125 %.** Le panneau tient sans défilement caché, et aucune carte n'est coupée. Mesure dans la tâche 4.
5. **Désinstallation.** L'installeur NSIS retire l'app et la clé `Run`, mais **jamais** `%APPDATA%\Oniris` ni `%LOCALAPPDATA%\Oniris` sans le demander. Vérification dans la tâche 7 et dans la liste de l'utilisateur.

---

### Task 1 : Démarrage et arrêt robustes

**Files :**
- Modify : `src-tauri/src/lib.rs` (`setup`, `RunEvent::Exit`), `src-tauri/src/logging.rs`, `crates/core/src/settings.rs`, `crates/core/src/db.rs`.

**Interfaces :**
- Si `AppState::open` échoue (base illisible, verrouillée ou dossier en lecture seule), un dialogue natif (`tauri-plugin-dialog`) affiche :
  - « Oniris n'a pas pu démarrer : {raison courte}. Le journal est dans {chemin du dossier logs}. »
  - puis l'app se ferme proprement.
  - La raison est une catégorie fixe : « base de données verrouillée », « base de données endommagée », « dossier inaccessible » ou « erreur inattendue ». Jamais le texte brut de l'erreur.
- À `RunEvent::Exit` : `PRAGMA wal_checkpoint(TRUNCATE)` sur la connexion, une seule fois, sans bloquer plus de 2 s. Puis `zenytt::shutdown()` comme aujourd'hui.
- `load_settings` re-valide **tous** les champs avec `fix()`, y compris `hover_delay_ms`, déjà validé à l'enregistrement. Le test le prouve sur une ligne JSON forgée.
- `pub fn startup_error_kind(e: &(dyn std::error::Error)) -> &'static str` dans `lib.rs` : fonction pure, testée.

- [ ] **Step 1 : Tests.**
  - `startup_error_kind` sur `SQLITE_BUSY`, `SQLITE_CORRUPT`/`NOTADB` et une erreur d'E/S « accès refusé ».
  - Checkpoint : une base temporaire en WAL ; après `checkpoint`, le fichier `-wal` fait 0 octet.
  - `load_settings` sur un JSON où `hover_delay_ms` vaut 50 : la valeur revient à son défaut.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `fix(app): explique les erreurs de démarrage et vide le WAL à la fermeture`

---

### Task 2 : Écran Paramètres

**Files :**
- Create : `src/modules/settings/{SettingsSheet.tsx,settings.css,settings.test.tsx}`.
- Modify :
  - `src/panel/Header.tsx` (bouton « Paramètres », avec la même logique `data-sheet` que les autres) ;
  - `src/panel/Panel.tsx` ;
  - `src/modules/projects/*` (« Projets masqués » avec « Ré-afficher ») ;
  - `src-tauri/src/tray.rs` (resynchroniser les cases cochées sur `oniris://settings`).

**Interfaces :**
- La feuille **Paramètres** a des sections. Chaque section a son bouton « Enregistrer » (`useSave` de `src/modules/iris/IrisSheet.tsx`, à déplacer dans `src/lib/useSave.ts` pour casser l'import circulaire relevé au plan 6). Chaque section se réinitialise **seulement** depuis ses propres champs.
  - **Général** : `user_name`, `autostart`, `reduce_motion`, `hide_pill_in_fullscreen`.
  - **Raccourci** : `hotkey` (capture d'une combinaison au clavier), `hold_threshold_ms`, `hover_open`, `hover_delay_ms`.
  - **Journée** : `timeline_start_hour`, `timeline_end_hour`, `focus_minutes`, `break_minutes`, `weather_city`.
  - **Presse-papiers** : `clipboard_enabled`, `clipboard_retention_days`, `clipboard_max_items`.
  - **Recherche et projets** : `search_roots`, `projects_roots` (listes éditables, avec un bouton « Ajouter un dossier » qui ouvre le sélecteur de `tauri-plugin-dialog`), `resume_prompt_normal`, `resume_prompt_orchestrator`, et la liste des projets masqués à ré-afficher.
  - **Mails** : `mail_poll_minutes`.
- Les réglages d'IRIS, de la voix, des comptes et des sites restent dans leurs feuilles. La feuille Paramètres renvoie vers elles par des liens.
- Un raccourci refusé par Rust (combinaison réservée) affiche le message sous le champ.

- [ ] **Step 1 : Tests (Vitest).**
  - Chaque section enregistre seulement ses champs, en relisant les réglages juste avant.
  - Une erreur Rust s'affiche sous la section.
  - Enregistrer « Journée » n'efface pas une saisie non enregistrée de « Général ».
  - Capture du raccourci : Ctrl+Maj+F5 donne `{ctrl, shift, key: "F5"}`.
  - Ré-afficher un projet masqué appelle `projects_hide(…, false)`.
  - Rust : le menu de la zone de notification suit un enregistrement fait depuis le panneau (presse-papiers, voix en pause, autostart).
  - Capture de la feuille.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(parametres): ajoute l'écran des paramètres`

---

### Task 3 : Premier lancement

**Files :**
- Create : `src/modules/onboarding/{Welcome.tsx,onboarding.css,onboarding.test.tsx}`, `src-tauri/src/modules/onboarding.rs`.
- Modify : `src-tauri/src/tray.rs` (`sync_autostart` derrière un trait `Autostart`), `lib.rs`, `build.rs` et `capabilities/panel.json`.

**Interfaces :**
- La clé `kv` `onboarding` vaut `{"step": u8, "done": bool, "autostart_consent": Option<bool>}`. Elle a deux commandes : `onboarding_get()` et `onboarding_set(state)`.
- Tant que `done` est faux :
  - le panneau s'ouvre sur l'écran d'accueil au lancement (une fois par lancement) ;
  - `sync_autostart` **n'écrit rien** dans la clé `Run`.
- Les écrans, chacun sautable sauf le 2 :
  1. « Bonjour ! Comment dois-je t'appeler ? » (`user_name`).
  2. « Lancer Oniris avec Windows ? », boutons Oui et Non. Ce choix règle `autostart` et `autostart_consent`.
  3. Dossiers de projets (proposition : `C:\Users\<toi>\Desktop\Utilitaires\Projets` s'il existe), puis dossiers de recherche.
  4. « Pour aller plus loin », avec des liens vers les feuilles Comptes, IRIS (Ollama) et Voix. Rien n'est installé ici.
  5. « C'est prêt », avec le raccourci Alt+Espace rappelé.
- Chaque étape enregistre `step`. Une reprise après fermeture revient à la dernière étape non terminée.

- [ ] **Step 1 : Tests.**
  - Rust, avec un faux `Autostart` : rien n'est écrit tant que `done` ou le consentement manque ; « Oui » écrit, « Non » retire.
  - Vitest : parcours complet ; reprise à l'étape 3 après remontage ; « Passer » ne saute jamais l'écran 2 ; le prénom est enregistré.
  - Capture de chaque écran.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(accueil): ajoute le premier lancement avec consentement au démarrage`

---

### Task 4 : Passe de performance et de mise en page côté interface

**Files :**
- Modify :
  - `src/panel/{CommandPalette.tsx,palette.css}` (`.pal-pop` monté seulement quand il y a des résultats) ;
  - `src/pill/Pill.tsx` (niveau de voix isolé dans un petit composant autour de `Orb`) ;
  - les CSS nécessaires à la tenue en 1080p.
- Create : `eslint.config.js` et le script `"lint": "eslint src"` dans `package.json`, avec `eslint-plugin-react-hooks` (règles `rules-of-hooks` et `exhaustive-deps` en `error`).

**Interfaces :**
- `pnpm lint` passe sans avertissement. Corriger les violations des hooks : leur correction est le but de ce point.
- Mesure par CDP à 2560×1440 et à 1920×1080 avec `deviceScaleFactor: 1.25` : la grille tient, sans `.more`, et aucune carte ne déborde.
  - Les chiffres sont enregistrés dans `tools/perf/layout-<date>.json`, par un petit script `tools/perf/layout.mjs`, réutilisable par l'utilisateur.
  - Si 1080p ne tient pas : densité réduite par `@media (max-height: 1100px)` (espacements, hauteur des cartes), sans masquer de carte.
- Capture des états clés aux deux tailles : panneau au repos, conversation, chaque feuille.

- [ ] **Step 1 :** Tests Vitest de non-régression (`.pal-pop` absent sans résultats ; la pilule ne se re-rend pas sur un événement de niveau, ce que prouve un compteur de rendus) ; `pnpm lint` ; mesures et captures.
- [ ] **Step 2 :** Commit : `perf(ui): allège la palette et la pilule, tient en 1080p, ajoute le lint des hooks`

---

### Task 5 : Passe de performance et de fiabilité côté cœur

**Files :** Modify :
- `crates/core/src/notes/*` et `src-tauri/src/modules/search.rs` (les notes de la recherche lues dans un cache mémoire reconstruit sur `Topic::Notes`, comme les favoris) ;
- `crates/core/src/search/*` (élagage de `usage` : entrées non utilisées depuis 180 jours, au démarrage, au plus une fois par jour) ;
- `src-tauri/src/modules/zenytt.rs` (lancement par `job::spawn`) ;
- `crates/core/src/music/*` (playlists Spotify effacées à la suppression du compte) ;
- `src-tauri/src/modules/iris.rs` (`unload_if_fullscreen` aussi sur un tour abandonné) ;
- `crates/core/src/voice/{assets.rs,stt.rs}` (`installed()` vérifie les fichiers `keep` ; `clean()` n'écarte que si le texte entier correspond à une hallucination, sans casse ni accents) ;
- `src-tauri/src/modules/voice/piper.rs` (`speak` ouvre la sortie dans `spawn_blocking`) ;
- `src/panel/Panel.tsx` (`focusHeaderButton` quand une confirmation ferme une feuille).

**Interfaces :** Les comportements ci-dessus, chacun avec son test.

- [ ] **Step 1 : Tests.**
  - Frappe dans la recherche : aucun verrou de base (la commande `search_query` reste sans `db.lock`, ce que prouve le test de lib.rs).
  - Élagage : 200 jours plus tard, l'entrée disparaît ; 10 jours plus tard, elle reste.
  - Zenytt : faux serveur MCP tué avec le Job.
  - Suppression d'un compte Spotify : le cache de playlists est vide.
  - Tour abandonné en plein écran : déchargement appelé.
  - `installed()` : `whisper\` présent sans `whisper-server.exe` donne « manquant ».
  - `clean("Active le sous-titrage")` passe, `clean("Sous-titrage ST' 501")` est écarté.
  - Focus rendu au bouton de l'en-tête.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `perf(app): allège la recherche et corrige les reports des plans 3 à 6`

---

### Task 6 : Outils de mesure pour l'utilisateur

**Files :**
- Create : `tools/perf/{soak.mjs,budgets.mjs}`.
- Modify : `tools/perf/README.md` et `tools/perf/measure.mjs`.

**Interfaces :**
- `node tools/perf/budgets.mjs` lance `measure.mjs` (ouverture, RAM, CPU) et y ajoute :
  - la VRAM au repos, par compteurs de performance Windows `GPU Process Memory(pid_*)\Dedicated Usage` via `Get-Counter` ;
  - le nombre de processus de voix et d'IA vivants (`whisper-server`, `piper`, `ollama runner`, `claude`) ;
  - un tableau **OK/HORS BUDGET** contre le §9 : ouverture p95 < 100 ms, CPU < 0,5 %, RAM < 200 Mo (repère), VRAM 0 Go.
- `node tools/perf/soak.mjs --hours 24` échantillonne toutes les 5 minutes la RAM privée et le CPU d'Oniris et de WebView2, en CSV.
  - Verdict à la fin : pente de la RAM en Mo/h par régression linéaire. **Fuite si la pente dépasse 1 Mo/h**, après exclusion de la première heure.
  - L'arrêt par Ctrl+C écrit le verdict partiel.
- Pas de dépendance nouvelle : Node standard, plus `powershell` pour `Get-Counter` et `Get-Process`.
- La régression linéaire et le verdict sont dans une fonction pure, exportée et testée par un petit test Node (`node --test tools/perf/soak.test.mjs`).

- [ ] **Step 1 :** Test de la régression (pente connue et données bruitées) ; README avec les procédures en français. **Ne lancer aucun des deux scripts sur la vraie machine.**
- [ ] **Step 2 :** Commit : `chore(perf): ajoute le verdict des budgets et le test de fuite sur 24 h`

---

### Task 7 : Empaquetage NSIS (configuration seulement)

**Files :** Modify : `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` (version), `package.json` (version). Create : `src-tauri/nsis/` si un gabarit est nécessaire.

**Interfaces :**
- `version` : `1.0.0` partout.
- `bundle` :
  - `targets: ["nsis"]` ;
  - `nsis: { installMode: "currentUser", languages: ["French"], displayLanguageSelector: false, compression: "lzma" }` ;
  - `shortDescription` et `longDescription` en français ;
  - `publisher` vide, ou « Oniris » sans nom de personne ;
  - `copyright` « © 2026 Oniris ».
- Désinstallation :
  - retirer la valeur `Run` d'Oniris (crochet `NSIS_HOOK_POSTUNINSTALL`, ou gabarit si besoin) ;
  - **ne pas** supprimer `%APPDATA%\Oniris` ni `%LOCALAPPDATA%\Oniris` ; ne demander que si le gabarit le permet proprement, sinon les laisser et le documenter.
- CSP vérifiée : rien n'est ajouté pour le build release, et les `iframe` des mails restent dans `sandbox` et `srcdoc`.
- **Aucun build n'est lancé.** Un test Rust lit `tauri.conf.json` et vérifie les valeurs ci-dessus (`installMode`, langue, version 1.0.0, cible nsis).

- [ ] **Step 1 :** Test de configuration, RED puis GREEN.
- [ ] **Step 2 :** Commit : `build(nsis): prépare l'installeur 1.0.0 par utilisateur`

---

### Task 8 : Documentation finale et liste de vérification globale

**Files :**
- Modify :
  - `docs/legal/confidentialite.md` (version 7 : autostart avec consentement ; journaux techniques et leur emplacement ; ce que l'installeur laisse à la désinstallation) ;
  - `docs/superpowers/specs/2026-11-03-oniris-design.md` (décisions datées : consentement, version 1.0.0, 1080p) ;
  - `docs/superpowers/plans/2026-11-03-oniris-feuille-de-route.md` (plan 7 ✅, plan 7b conditionnel aux mesures, plan 8 optionnel).
- Create :
  - `docs/verifications/plan-07-finitions.md` ;
  - `README.md` à la racine (s'il n'existe pas), en français : ce qu'est Oniris, prérequis, installation, premier lancement, données et confidentialité, mesures.

**Contenu de la liste :**
1. **Construire l'installeur sur accord.** Commandes exactes : `pnpm build`, puis `pnpm tauri build`.
2. Installer, puis faire le premier lancement complet.
3. `node tools/perf/budgets.mjs`, avec le tableau attendu.
4. `node tools/perf/soak.mjs --hours 24`.
5. Désinstaller, et vérifier que la clé `Run` a disparu et que les dossiers de données sont restés.
6. Rappel des listes de vérification 1 à 6, encore à faire.

- [ ] **Step 1 :** Documents, puis toute la suite au vert : `cargo test --workspace`, clippy `-D warnings`, `pnpm test`, `pnpm lint` et `pnpm build`.
- [ ] **Step 2 :** Commit : `docs(finitions): met à jour confidentialité, spec, feuille de route et guide d'installation`
