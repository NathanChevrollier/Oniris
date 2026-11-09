# Oniris — Plan 8 : Mot d'activation « Iris »

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** On dit « Iris », puis sa demande, sans toucher au clavier. IRIS traite la demande comme un push-to-talk. L'écoute continue est désactivée par défaut et ne transcrit ni n'envoie rien avant la détection.

**Architecture :** Le plan a deux parties.

- **Partie A, entraînement :** un modèle openWakeWord « Iris » entraîné hors de l'app, par l'utilisateur, avec le kit `tools/wakeword/`. Cette partie est manuelle et ne passe pas par des sous-agents (règle R8).
- **Partie B, intégration.** Oniris embarque trois petits modèles ONNX : mel, embedding et le classifieur `iris.onnx`.
  - Un **détecteur en Rust pur** (`tract-onnx`, sans DLL) les exécute en flux, par pas de 80 ms.
  - Un **écouteur** possède un flux micro continu. À la détection, il bascule ce même flux en enregistrement, sans trou : la demande qui suit « Iris » est captée en entier.
  - Un **détecteur de fin de parole** (énergie) ferme l'enregistrement.
  - L'enregistrement passe ensuite par `Pipeline::process`, comme un push-to-talk.

**Tech Stack :** Celle des plans 1 à 7, plus `tract-onnx` (MIT/Apache-2.0) dans `oniris-core`. Côté entraînement : Python 3.10 dans WSL, openWakeWord, piper-tts 1.2.0 et PyTorch CPU (voir `tools/wakeword/requirements.txt`).

**Spec :** [`docs/superpowers/specs/2026-11-03-oniris-design.md`](../specs/2026-11-03-oniris-design.md), §4.3 (mot d'activation), §7.2 (interruption), §9 (CPU au repos). Feuille de route : ligne 8.

**Runbook de la partie A :** [`tools/wakeword/README.md`](../../../tools/wakeword/README.md).

## Décisions prises avant ce plan (2026-11-09)

- **Moteur d'inférence : `tract-onnx`** plutôt que `ort`.
  - Raison : pas de DLL onnxruntime à livrer, et du Rust pur, comme le reste.
  - Le crate `livekit-wakeword` exécute déjà les mêmes `melspectrogram.onnx` et `embedding_model.onnx` via `ort-tract` : c'est la preuve que tract sait lire ces modèles.
  - On ne prend pas `livekit-wakeword` lui-même : il recalcule 2 s d'audio à chaque appel, soit environ 25 fois trop de calcul en continu.
  - **Repli** si tract échoue sur un modèle (Tâche 1, étape 2) : `ort` 2.x avec `download-binaries`. C'est un choix à faire valider par l'utilisateur.
- **Un seul flux micro.** L'écoute et l'enregistrement de la demande partagent le flux. Pendant un push-to-talk, l'écouteur se suspend, puis reprend à la fin du tour.
- **Suspendu quand :**
  - la voix est en pause (menu de la zone de notification) ;
  - la voix n'est pas installée ;
  - une application est en plein écran ;
  - IRIS parle (pour éviter qu'elle se déclenche elle-même) ;
  - un tour est en cours ;
  - le réglage est désactivé ;
  - les modèles sont absents ou corrompus.

  L'interruption par la voix n'existe pas : on interrompt IRIS avec Alt+Espace ou Échap, comme au plan 6.
- **Aucun son de confirmation.** À la détection, la pilule passe en « Écoute » : la sphère s'anime, comme au push-to-talk.
- **Modèles livrés dans l'installateur** (`src-tauri/resources/wake/`, environ 2 Mo), vérifiés SHA-256 au chargement. On ne les télécharge pas : ils sont à nous et petits.
- **Seuil :** une constante `WAKE_THRESHOLD`, fixée par l'évaluation de la partie A. Pas de réglage de sensibilité dans l'interface (YAGNI) : si les faux déclenchements gênent, on réentraîne.
- **Période réfractaire :** 2 s après une détection.

## Global Constraints

- Le mot d'activation est **désactivé par défaut** (`voice_wake: false`).
- Avant la détection, l'audio ne vit qu'en mémoire, dans les tampons du détecteur (au plus 2 s). Il n'est **jamais** écrit sur le disque, transcrit, journalisé ni envoyé.
- Après la détection, l'audio suit les règles du push-to-talk : 30 s au plus, en mémoire, vers Whisper sur 127.0.0.1 seulement.
- Zéro coût et zéro réseau à l'exécution.
- Le push-to-talk reste la voie principale. Il garde la priorité sur l'écoute et ne doit pas régresser : les tests du plan 6 restent verts.
- CPU avec l'écoute active, l'app au repos : **≤ 1 % du processeur total**, en moyenne sur 60 s (§9). Le détecteur traite 60 s d'audio en **moins de 3 s** sur un cœur.
- Les textes de l'interface sont en français et tutoient l'utilisateur, comme le reste de l'app.
- Règles R8 pour les sous-agents :
  - jamais de vrai micro (tests `#[ignore]` seulement) ;
  - jamais `tauri dev` ni `tauri build` ;
  - pas de réseau.

  Les modèles de `src-tauri/resources/wake/` et la fixture sont des fichiers du dépôt, produits par la partie A.

## Review Focus

1. **Le micro disparaît ou est pris par une autre app pendant l'écoute** (casque débranché, veille du PC). L'écouteur se rouvre avec une attente croissante (1 s, 2 s, 4 s… jusqu'à 60 s), avec **un seul** toast par panne, et jamais en boucle serrée. Test en Tâche 4.
2. **IRIS dit « Iris » à voix haute** (« Je suis Iris… »). Elle ne doit pas se déclencher elle-même : l'écouteur est suspendu pendant `Speaking` et 500 ms après. Test en Tâche 4.
3. **Désactiver le réglage, ou mettre la voix en pause, au milieu d'une demande.** Le micro se ferme tout de suite, l'audio est jeté, et aucun tour ne part. Test en Tâche 4.
4. **Alt+Espace pressé alors que l'écoute est armée ou en train d'enregistrer après « Iris ».** Le push-to-talk gagne : l'enregistrement par mot est abandonné sans tour, et le micro passe au push-to-talk. Test en Tâche 4.
5. **« Iris » suivi de rien** (déclenchement à tort, ou l'utilisateur se tait). Après 4 s sans parole, retour au repos sans transcription ni toast. Test en Tâche 3 (fin de parole) et en Tâche 4 (aucun tour).

---

## Partie A — Entraînement (manuel, avec l'utilisateur)

### Tâche 0 : Entraîner, évaluer et livrer le modèle

Ce n'est pas une tâche de sous-agent. L'utilisateur la mène avec le contrôleur, en suivant [`tools/wakeword/README.md`](../../../tools/wakeword/README.md).

- [ ] Sous Windows, `record_samples.py` : 60 « Iris » et 5 min d'ambiance.
- [ ] Dans WSL, `setup.sh` s'est terminé sans erreur, et `versions.txt` existe.
- [ ] `train.sh` a produit `~/oniris-wakeword/output/iris.onnx`.
- [ ] `train.sh fixture` a créé :
  - `src-tauri/resources/wake/{melspectrogram,embedding_model,iris}.onnx` et `SHA256SUMS` ;
  - `src-tauri/tests/fixtures/wake/{sequence.wav,scores.json}`.
- [ ] L'écoute en direct (`listen_live.py`) atteint l'objectif, avec si besoin des cycles `train.sh retrain` :
  - **au moins 3 h** de vie normale à au plus 0,1 faux déclenchement par heure ;
  - au moins 90 % de rappel en mode `--test`.

  Noter le seuil et les bilans (`sessions.jsonl`) dans le journal du plan.
- [ ] Commit : `feat(voix): ajoute le modèle du mot d'activation « Iris »`, avec les modèles, la fixture et le seuil noté dans le journal.

**Si l'objectif n'est pas atteint :** appliquer la section « Si le modèle est mauvais » du README. Passer à « Dis Iris » demande l'accord de l'utilisateur. La partie B ne commence qu'avec un modèle qui atteint l'objectif.

---

## Partie B — Intégration dans Oniris

### Tâche 1 : Détecteur en flux (`oniris-core`)

**Files:**
- Create: `crates/core/src/voice/wake.rs`
- Modify: `crates/core/src/voice/mod.rs` (ajout de `pub mod wake;`), `crates/core/Cargo.toml` (`tract-onnx`)
- Test: `crates/core/src/voice/wake.rs` (module `tests`), qui lit `src-tauri/tests/fixtures/wake/` et `src-tauri/resources/wake/` via `env!("CARGO_MANIFEST_DIR")/../../src-tauri/...`

**Interfaces:**
- Produces:
  - `pub struct WakeModels { pub mel: PathBuf, pub embedding: PathBuf, pub classifier: PathBuf }`
  - `impl WakeModels { pub fn in_dir(dir: &Path) -> Self }` : les noms `melspectrogram.onnx`, `embedding_model.onnx` et `iris.onnx`.
  - `pub fn verify(dir: &Path) -> Result<(), WakeError>` : compare chaque fichier à `SHA256SUMS`.
  - `pub enum WakeError { Missing(String), Corrupt(String), Model(String) }`, avec `Display` en français.
  - `pub struct WakeDetector`
    - `pub fn load(m: &WakeModels) -> Result<Self, WakeError>`
    - `pub fn push(&mut self, samples_16k: &[f32]) -> Vec<f32>` : accepte n'importe quelle longueur et rend un score par pas complet de 1 280 échantillons.
    - `pub fn reset(&mut self)`
  - `pub const WAKE_CHUNK: usize = 1280;`

- [ ] **Step 1 : écrire les tests qui échouent**

```rust
#[test] fn fixture_scores_match_python() {
    // sequence.wav (i16) → f32 / 32768 ; push par morceaux de 1 000 échantillons (pas alignés sur 1 280)
    // pour i >= fx.compare_from : (rust[i] - fx.scores[i]).abs() <= 0.02
    // et argmax(rust) == argmax(fx.scores)
}
#[test] fn silence_never_triggers() { /* 10 s de zéros → max < 0.1 */ }
#[test] fn verify_detects_corruption() { /* copie dans un tempdir, un octet modifié → Err(WakeError::Corrupt(_)) */ }
#[test] fn missing_model_is_reported() { /* dossier vide → Err(WakeError::Missing(_)) */ }
#[test] #[ignore] fn sixty_seconds_under_three() { /* 60 s de bruit blanc faible : push complet en < 3 s (build --release) */ }
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** Lancer `cargo test -p oniris-core voice::wake`. Attendu : ÉCHEC, `WakeDetector` est introuvable.

  Si, une fois écrit, tract refuse un des modèles (opérateur non pris en charge), **s'arrêter et remonter l'erreur** : le repli `ort` demande l'accord de l'utilisateur.
- [ ] **Step 3 : écrire `WakeDetector`**
  - Reproduire **exactement** le flux de `AudioFeatures` dans `openwakeword/utils.py`, au commit noté dans `scores.json` (`openwakeword`) :
    - l'entrée du modèle mel est en **valeurs i16 converties en f32**, non normalisées ;
    - à la sortie : `spec / 10 + 2` ;
    - à chaque pas de 1 280 échantillons, le mel est calculé sur les 1 280 + 480 derniers échantillons ;
    - l'embedding est calculé sur les 76 dernières trames mel (`[1, 76, 32, 1]` → 96 valeurs) ;
    - le classifieur reçoit les 16 derniers embeddings (`[1, 16, 96]`).
  - Les tampons sont bornés : 10 s de mel et 120 embeddings au plus.
  - Les modèles sont chargés une fois, avec `into_optimized().into_runnable()`.
  - La fixture est l'arbitre : en cas de doute, c'est elle qui tranche, pas cette description.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** `cargo test -p oniris-core voice::wake`, puis `cargo test -p oniris-core --release voice::wake -- --ignored`. Attendu : PASS, et le test des 60 s affiche sa durée.
- [ ] **Step 5 : commit.** `feat(voix): détecteur du mot d'activation en Rust pur`

### Tâche 2 : Capture en flux

**Files:**
- Modify: `crates/core/src/voice/audio.rs` (ajout de `Resampler16k`)
- Modify: `src-tauri/src/modules/voice/capture.rs` (ajout de `Mic::listen`)
- Test: les modules `tests` des deux fichiers

**Interfaces:**
- Produces:
  - `pub struct Resampler16k`
    - `pub fn new(rate: u32, channels: u16) -> Self`
    - `pub fn push(&mut self, interleaved: &[f32], out: &mut Vec<f32>)` : mono 16 kHz, avec un état conservé entre les appels.
  - `trait Mic` gagne `fn listen(&self, device: &str, sink: Box<dyn FnMut(&[f32]) + Send>) -> Result<Box<dyn Listening>, MicError>;`
  - `pub trait Listening: Send { fn stop(self: Box<Self>); fn failed(&self) -> bool; }` : le micro est fermé au retour de `stop` ; `failed` passe à vrai si le flux cpal signale une erreur (périphérique perdu).
  - `sink` reçoit du mono 16 kHz, en morceaux de taille quelconque, sur le thread audio. Il ne doit ni bloquer ni allouer à chaque appel.
  - Les faux micros existants des tests (`pipeline.rs`, `capture.rs`) implémentent `listen`.

- [ ] **Step 1 : écrire les tests qui échouent**

```rust
#[test] fn streaming_equals_one_shot() {
    // sinus 440 Hz, 48 kHz stéréo, 2 s, découpé en morceaux de tailles 1, 7, 480, 1023, 4800 en alternance
    // concat(push) vs resample_to_16k(tout) : même longueur à ±1, écart max <= 1e-4
}
#[test] fn sixteen_k_mono_passthrough() { /* rate 16000, 1 canal : sortie == entrée */ }
#[test] #[ignore] fn vrai_micro_flux_deux_secondes() { /* listen 2 s : ≥ 30 000 échantillons reçus */ }
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris-core voice::audio` puis `cargo test -p oniris voice::capture`. Attendu : ÉCHEC à la compilation.
- [ ] **Step 3 : écrire `Resampler16k` et `CpalMic::listen`**
  - `Resampler16k` utilise la même interpolation linéaire que `resample_to_16k`. Il garde la position fractionnaire et le dernier échantillon mono entre deux appels.
  - `listen` réutilise `find_device` et le choix du format de `open`. Le callback cpal convertit, rééchantillonne dans un `Vec` réutilisé, puis appelle `sink`.
  - Les erreurs du flux (`|e| ...`) positionnent un `AtomicBool` partagé, rendu par `Listening::failed`.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** Mêmes commandes. Attendu : PASS. Les tests `#[ignore]` ne sont pas lancés.
- [ ] **Step 5 : commit.** `feat(voix): capture micro en flux continu`

### Tâche 3 : Fin de parole

**Files:**
- Create: `crates/core/src/voice/endpoint.rs`
- Modify: `crates/core/src/voice/mod.rs`
- Test: `crates/core/src/voice/endpoint.rs`

**Interfaces:**
- Produces:
  - `pub enum Endpoint { Continue, End, NoSpeech, TooLong }`
  - `pub struct Endpointer`
    - `pub fn new() -> Self`
    - `pub fn push(&mut self, samples_16k: &[f32]) -> Endpoint`
  - Constantes :
    - `SPEECH_START_MS = 200`
    - `END_SILENCE_MS = 800`
    - `NO_SPEECH_MS = 4000`
    - `MAX_UTTERANCE_MS = 15000`
  - Trames de 30 ms. Une trame est de la parole si `rms > max(0.01, 3 × plancher de bruit)`. Le plancher de bruit est le minimum glissant du RMS sur les 1 s précédentes, amorcé avec les 300 premières millisecondes.

- [ ] **Step 1 : écrire les tests qui échouent**

```rust
#[test] fn speech_then_silence_ends()      { /* 0,3 s bruit 0,005 + 1,5 s sinus 0,2 + 1 s bruit 0,005 → End, entre 0,8 et 0,9 s après la fin du sinus */ }
#[test] fn nothing_said_is_no_speech()     { /* 5 s de bruit 0,005 → NoSpeech à 4 s ±30 ms, jamais End */ }
#[test] fn endless_speech_is_too_long()    { /* 16 s de sinus → TooLong à 15 s */ }
#[test] fn short_click_is_not_speech()     { /* 0,1 s de sinus au milieu de 5 s de bruit → NoSpeech */ }
#[test] fn loud_room_still_ends()          { /* bruit 0,05 + parole 0,4 → End (plancher adaptatif) */ }
#[test] fn chunk_size_does_not_matter()    { /* même signal par morceaux de 160 ou de 4 800 → même événement, à 30 ms près */ }
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris-core voice::endpoint`. Attendu : ÉCHEC à la compilation.
- [ ] **Step 3 : écrire `Endpointer`**, avec les valeurs ci-dessus.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** Même commande. Attendu : PASS.
- [ ] **Step 5 : commit.** `feat(voix): détection de fin de parole`

### Tâche 4 : Écouteur et branchement sur le pipeline

**Files:**
- Create: `src-tauri/src/modules/voice/wake.rs`
- Modify: `src-tauri/src/modules/voice/mod.rs`, `src-tauri/src/modules/voice/pipeline.rs`, `src-tauri/src/lib.rs` (démarrage), `src-tauri/src/tray.rs` (la pause suspend aussi l'écoute)
- Test: `src-tauri/src/modules/voice/wake.rs` et `pipeline.rs` (modules `tests`, faux micro, faux détecteur)

**Interfaces:**
- Consumes :
  - de la Tâche 1 : `WakeDetector`, `WakeModels`, `verify`, `WAKE_CHUNK` ;
  - de la Tâche 2 : `Mic::listen`, `Listening` ;
  - de la Tâche 3 : `Endpointer`, `Endpoint`.
- Produces :
  - `pub trait Detector: Send { fn push(&mut self, s: &[f32]) -> Vec<f32>; fn reset(&mut self); }`, implémenté pour `WakeDetector` et pour un faux détecteur de test.
  - `pub const WAKE_THRESHOLD: f32` = la valeur notée en Tâche 0.
  - `pub const REFRACTORY: Duration = 2 s`
  - `pub const AFTER_SPEECH: Duration = 500 ms`
  - Dans `pipeline.rs` :
    - `Input` gagne `Wake` et `WakeEnd(u64, Vec<f32>)` ;
    - `Pipeline::wake_begin(self: &Arc<Self>) -> Option<u64>` : interruption de la voix, mêmes vérifications que `start` (pause, installation), phase `Listening`, et rend le numéro d'appui. Elle n'ouvre **pas** le micro.
    - `Conf` gagne `wake: bool`.
  - `pub struct WakeListener` :
    - `pub fn start(app: &AppHandle)` ;
    - un thread qui suit la machine à états `Off → Armed → Recording(press) → Armed`, plus `Suspended(raison)` et `Failed { retry_at, backoff }`.
    - `pub fn wake_state() -> &'static str`, pour la feuille IRIS : « désactivé », « à l'écoute », « en pause », « micro indisponible », « modèle absent ».

- [ ] **Step 1 : écrire les tests qui échouent** (`wake.rs`, avec le faux micro qui pousse des morceaux à la demande et le faux détecteur qui rend des scores scriptés)

```rust
#[tokio::test] async fn off_by_default_never_opens_mic()          { /* Conf.wake=false : listen jamais appelé */ }
#[tokio::test] async fn nothing_reaches_stt_before_detection()    { /* 30 s de scores 0.1 : 0 appel Stt, 0 Input envoyé */ }
#[tokio::test] async fn detection_records_then_processes()        { /* score 0.9 → Input::Wake ; parole puis silence → Input::WakeEnd(press, s) avec s.len() > 0 ; l'audio d'avant la détection n'est pas dans s */ }
#[tokio::test] async fn refractory_two_seconds()                  { /* deux pics à 1 s d'écart → une seule détection */ }
#[tokio::test] async fn suspended_while_speaking_and_after()      { /* SPEAKING=true puis false : pic à +300 ms ignoré, pic à +700 ms détecté */ }
#[tokio::test] async fn disabling_mid_recording_discards()        { /* wake=false pendant Recording : stop() appelé, pas de WakeEnd */ }
#[tokio::test] async fn push_to_talk_preempts_recording()         { /* Input::Start pendant Recording : enregistrement abandonné, listen stoppé avant Mic::start */ }
#[tokio::test] async fn no_speech_after_wake_is_silent()          { /* détection puis 5 s de bruit : retour à Armed, pas de WakeEnd, pas de toast */ }
#[tokio::test] async fn mic_failure_backs_off_with_one_toast()    { /* listen échoue 5 fois : tentatives à 1, 2, 4, 8, 16 s (horloge tokio en pause) ; un seul toast */ }
#[tokio::test] async fn fullscreen_or_pause_suspends()            { /* Conf.fullscreen / paused → micro fermé ; réarmé quand ça repasse à false */ }
#[tokio::test] async fn missing_models_disable_cleanly()          { /* verify → Missing : état « modèle absent », micro jamais ouvert */ }
```

  Dans `pipeline.rs`, ajouter `wake_turn_goes_through_process`. Il vérifie que `wake_begin` suivi de `process` donne les mêmes toast et voix que le test `reponse_courte_toast_puis_voix`.
- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris voice::`. Attendu : ÉCHEC à la compilation.
- [ ] **Step 3 : écrire `WakeListener` et brancher le pipeline**
  - L'état est relu à chaque pas de 80 ms par le thread de l'écouteur. Le callback micro pousse dans un canal borné (`crossbeam` ou `std::sync::mpsc::sync_channel(64)`) : pas de travail lourd sur le thread audio.
  - Pendant `Recording`, l'audio va dans un `Vec` plafonné à 30 s et dans l'`Endpointer`. Il ne va plus au détecteur.
    - `End` ou `TooLong` → `Input::WakeEnd`.
    - `NoSpeech` → retour à `Armed`, sans rien.
  - Le pipeline traite `WakeEnd(press, s)` avec le `process` existant, seulement si `press` est encore l'appui courant.
  - `Input::Start` (push-to-talk) envoie d'abord un signal de suspension à l'écouteur et attend la fermeture du flux, 300 ms au plus, avant `Mic::start`. L'écouteur se réarme quand la phase revient à `Idle`.
  - **Rien** ne journalise d'audio ni de score au-dessus du niveau `debug`. Une ligne `info` est écrite par détection : « mot d'activation détecté », sans le score.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** `cargo test -p oniris`. Attendu : PASS, y compris les 20 tests existants du pipeline du plan 6.
- [ ] **Step 5 : commit.** `feat(voix): écoute du mot d'activation branchée sur IRIS`

### Tâche 5 : Réglage, interface, livraison et documents

**Files:**
- Modify :
  - `crates/core/src/settings.rs` (`voice_wake: bool`, défaut `false`) ;
  - `src/lib/settings.ts` (type) ;
  - `src/modules/voice/VoiceSection.tsx` ;
  - `src/modules/voice/voice.test.tsx` ;
  - `src-tauri/tauri.conf.json` (`bundle.resources` : `"resources/wake/*"`) ;
  - `src-tauri/tests/nsis_config.rs` ;
  - `docs/legal/confidentialite.md` (v8) ;
  - la spec, §4.3 ;
  - `docs/superpowers/plans/2026-11-03-oniris-feuille-de-route.md` (plan 8 ✅) ;
  - `README.md`.
- Create: `docs/verifications/plan-08-mot-activation.md`

**Interfaces:**
- Consumes: `wake_state()` (Tâche 4) via une commande `voice_wake_state() -> String`, ajoutée dans `modules/voice/pipeline.rs` à côté de `status_label`.

- [ ] **Step 1 : écrire les tests qui échouent**

```rust
// settings.rs
#[test] fn voice_wake_defaults_to_false() { assert!(!Settings::default().voice_wake); }
// nsis_config.rs
#[test] fn wake_models_are_bundled() { /* bundle.resources contient "resources/wake/*" */ }
```
```tsx
// voice.test.tsx
it("le mot d'activation est décoché par défaut et s'enregistre", ...)  // libellé « Mot d'activation « Iris » » ; coché + Enregistrer → save({voice_wake: true})
it("grisé tant que la voix n'est pas installée", ...)
it("affiche l'état de l'écoute", ...) // voice_wake_state = "à l'écoute" → texte visible
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris-core settings`, `cargo test -p oniris --test nsis_config`, puis `pnpm vitest run src/modules/voice`. Attendu : ÉCHEC.
- [ ] **Step 3 : écrire le code**
  - La case reprend le texte exact suivant, sous elle, en `voice-note` :

    > « Oniris écoute en continu et réagit quand tu dis « Iris ». Avant ça, rien n'est transcrit, enregistré ni envoyé. Windows affiche l'icône du micro tant que l'écoute est active. »

  - L'état est affiché à côté de la case : `voice_wake_state`.
  - `confidentialite.md` v8 : la section « Mot d'activation » remplace la ligne « à venir ». Elle dit :
    - ce qui est écouté ;
    - les 2 s en mémoire ;
    - aucune écriture ;
    - les suspensions (pause, plein écran, IRIS qui parle) ;
    - que le modèle est entraîné sur des voix synthétiques et, si l'utilisateur l'a fait, sur ses propres enregistrements, restés sur son PC.
  - La spec, §4.3, gagne la mention « [décision du 2026-11-09] livré au plan 8 », avec le seuil retenu.
  - La liste de vérification couvre :
    - activer le réglage, puis dire « Iris, quelle heure est-il ? » à 1 m et à 3 m ;
    - une soirée de jeu en plein écran, sans aucune détection ;
    - une heure de musique et de vidéo avec l'écoute active, en comptant les faux déclenchements ;
    - le CPU au repos avec l'écoute active, avec `tools/perf/budgets.mjs` et le gestionnaire des tâches (≤ 1 %) ;
    - débrancher, puis rebrancher le casque ;
    - la mise en pause depuis la zone de notification.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** Mêmes commandes, plus `pnpm lint` et `pnpm build`. Attendu : tout est vert.
- [ ] **Step 5 : commit.** `feat(voix): réglage du mot d'activation et documents`

---

## Après la partie B

- Revue finale de toute la branche, puis fusion `--ff-only` et `graphify update .`, comme aux plans 5 à 7.
- **Nouveau build de l'installateur (1.1.0) seulement avec l'accord de l'utilisateur**, puis la liste de vérification du plan 8.
