# Oniris — Plan 8 : Mot d'activation « Iris »

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** On dit « Dis Iris » ou « Hé Iris », puis sa demande, sans toucher au clavier. IRIS traite la demande comme un push-to-talk. L'écoute continue est désactivée par défaut et ne transcrit ni n'envoie rien avant la détection.

**Architecture :** Le plan a deux parties.

- **Partie A, entraînement :** un modèle openWakeWord « Iris » entraîné hors de l'app, par l'utilisateur, avec le kit `tools/wakeword/`. Cette partie est manuelle et ne passe pas par des sous-agents (règle R8).
- **Partie B, intégration.** Oniris embarque trois petits modèles ONNX : mel, embedding et le classifieur `dis_iris.onnx`.
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

## Décisions du 2026-11-12 (amendement : intégrer maintenant, apprendre en usage)

Elles remplacent les décisions ci-dessus quand elles les contredisent.

- **Modèle livré : `dis_iris.onnx`**, une seule entrée qui reconnaît « Dis Iris » et « Hé Iris ». Variante `L128_N1000_P300` de la recherche de réglages du 12/11.
- **« Iris » seul est désactivé.** `iris.onnx` n'est pas livré : sans faux déclenchements, il ne reconnaissait qu'environ 1 « Iris » sur 4.
- **La partie B commence avec un modèle provisoire,** qui n'atteint pas encore l'objectif. Mesures du 12/11 au seuil retenu :
  - ≈ 2 faux déclenchements par heure ;
  - ≈ 50 % de la voix de l'utilisateur reconnue, mesuré sur 12 clips seulement, donc très imprécis ;
  - 78 % des clips synthétiques propres reconnus.

  L'objectif (au plus 0,1 faux déclenchement par heure et au moins 90 % de reconnaissance) devient celui du **mode apprentissage**, Tâches 6 et 7.
- **`WAKE_THRESHOLD = 0.998`.** Les scores de ce modèle sont saturés près de 1 : c'est l'analyse fine du 12/11 (`~/oniris-wakeword/output/analyse_fine.json`) qui a fixé ce seuil.
- **Mode apprentissage**, choisi par l'utilisateur :
  - désactivé par défaut ;
  - il garde sur le disque local des extraits étiquetés, qui servent au réentraînement du seul modèle principal ;
  - étiquetage automatique d'après le comportement, plus deux boutons dans la pilule : « Je vais dire Iris » (positif certain) et « Faux réveil » (négatif certain) ;
  - les boutons l'emportent sur l'étiquetage automatique.

## Global Constraints

- Le mot d'activation est **désactivé par défaut** (`voice_wake: false`).
- Avant la détection, l'audio ne vit qu'en mémoire, dans les tampons du détecteur (au plus 2 s). Il n'est **jamais** écrit sur le disque, transcrit, journalisé ni envoyé.
  - **Seule exception : le mode apprentissage** (`voice_wake_learning`, désactivé par défaut, réglage distinct).
  - Il écrit des extraits de 3 s au plus, seulement autour d'un réveil, d'un presque-réveil ou d'un appui sur « Je vais dire Iris ».
  - Les extraits vont dans `%USERPROFILE%\oniris-wakeword\recordings\learn\{pos,neg}\` et nulle part ailleurs.
  - Ils ne sont jamais transcrits ni envoyés.
  - Désactiver le mode n'efface rien, c'est dit dans l'interface. Un bouton « Supprimer les extraits » vide le dossier.
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
6. **Mode apprentissage, l'utilisateur appuie sur « Je vais dire Iris » puis ne dit rien,** ou parle d'autre chose. Après la fenêtre, l'extrait est gardé comme positif autour du meilleur score, **seulement si ce score dépasse 0,05**. Sinon rien n'est écrit, car ce serait un « positif » vide qui empoisonnerait l'entraînement. Test en Tâche 6.
7. **Mode apprentissage et disque :** au-delà de 2 000 extraits ou de 200 Mo, les plus anciens sont supprimés. Le dossier ne grossit jamais sans limite. Test en Tâche 6.

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

**Si l'objectif n'est pas atteint :** appliquer la section « Si le modèle est mauvais » du README.

**Amendement du 2026-11-12 :** l'objectif n'est pas atteint, et l'utilisateur a choisi de l'atteindre en usage, avec le mode apprentissage. La Tâche 0 bis remplace donc les deux dernières cases ci-dessus.

### Tâche 0 bis : Livrer le modèle provisoire (contrôleur, pas de sous-agent)

- [ ] Dans WSL, rendre la variante choisie officielle, puis refaire la fixture :
  ```bash
  cp ~/oniris-wakeword/output/sweep/dis_iris_L128_N1000_P300.onnx ~/oniris-wakeword/output/dis_iris.onnx
  MODEL=dis_iris bash tools/wakeword/train.sh fixture
  ```
- [ ] Retirer du dépôt de travail :
  - `src-tauri/resources/wake/iris.onnx` ;
  - `src-tauri/tests/fixtures/wake/iris/` ;
  - les anciens `src-tauri/tests/fixtures/wake/{sequence.wav,scores.json}`, restés à la racine.

  Puis régénérer `SHA256SUMS` sans `iris.onnx` : `make_fixture.py` le refait à partir des `*.onnx` présents.
- [ ] Commit : `feat(voix): ajoute le modèle provisoire « Dis Iris »`, avec les modèles, la fixture `wake/dis_iris/` et, dans le message, le seuil 0,998 et les mesures du 12/11.

---

## Partie B — Intégration dans Oniris

### Tâche 1 : Détecteur en flux (`oniris-core`)

**Files:**
- Create: `crates/core/src/voice/wake.rs`
- Modify: `crates/core/src/voice/mod.rs` (ajout de `pub mod wake;`), `crates/core/Cargo.toml` (`tract-onnx`)
- Test: `crates/core/src/voice/wake.rs` (module `tests`), qui lit `src-tauri/tests/fixtures/wake/dis_iris/` et `src-tauri/resources/wake/` via `env!("CARGO_MANIFEST_DIR")/../../src-tauri/...`

**Interfaces:**
- Produces:
  - `pub struct WakeModels { pub mel: PathBuf, pub embedding: PathBuf, pub classifier: PathBuf }`
  - `impl WakeModels { pub fn in_dir(dir: &Path) -> Self }` : les noms `melspectrogram.onnx`, `embedding_model.onnx` et `dis_iris.onnx`.
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
  - `pub const WAKE_THRESHOLD: f32 = 0.998;` (Tâche 0 bis).
  - `pub const NEAR_MISS: f32 = 0.3;` : un « presque-réveil », utilisé seulement par le mode apprentissage (Tâche 6).
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
  - `docs/legal/confidentialite.md` (v9) ;
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
it("le mot d'activation est décoché par défaut et s'enregistre", ...)  // libellé « Mot d'activation « Dis Iris » » ; coché + Enregistrer → save({voice_wake: true})
it("grisé tant que la voix n'est pas installée", ...)
it("affiche l'état de l'écoute", ...) // voice_wake_state = "à l'écoute" → texte visible
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris-core settings`, `cargo test -p oniris --test nsis_config`, puis `pnpm vitest run src/modules/voice`. Attendu : ÉCHEC.
- [ ] **Step 3 : écrire le code**
  - La case reprend le texte exact suivant, sous elle, en `voice-note` :

    > « Oniris écoute en continu et réagit quand tu dis « Dis Iris » ou « Hé Iris ». Avant ça, rien n'est transcrit, enregistré ni envoyé. Windows affiche l'icône du micro tant que l'écoute est active. »

  - L'état est affiché à côté de la case : `voice_wake_state`.
  - `confidentialite.md` v9 : la section « Mot d'activation » remplace la ligne « à venir ». Le mode apprentissage s'y ajoute en Tâche 7. Elle dit :
    - ce qui est écouté ;
    - les 2 s en mémoire ;
    - aucune écriture ;
    - les suspensions (pause, plein écran, IRIS qui parle) ;
    - que le modèle est entraîné sur des voix synthétiques et, si l'utilisateur l'a fait, sur ses propres enregistrements, restés sur son PC.
  - La spec, §4.3, gagne la mention « [décision du 2026-11-09] livré au plan 8 », avec le seuil retenu.
  - La liste de vérification couvre :
    - activer le réglage, puis dire « Dis Iris, quelle heure est-il ? » à 1 m et à 3 m ;
    - une soirée de jeu en plein écran, sans aucune détection ;
    - une heure de musique et de vidéo avec l'écoute active, en comptant les faux déclenchements ;
    - le CPU au repos avec l'écoute active, avec `tools/perf/budgets.mjs` et le gestionnaire des tâches (≤ 1 %) ;
    - débrancher, puis rebrancher le casque ;
    - la mise en pause depuis la zone de notification.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** Mêmes commandes, plus `pnpm lint` et `pnpm build`. Attendu : tout est vert.
- [ ] **Step 5 : commit.** `feat(voix): réglage du mot d'activation et documents`

### Tâche 6 : Mode apprentissage, collecte étiquetée (Rust)

**Files:**
- Create: `src-tauri/src/modules/voice/learn.rs`
- Modify:
  - `src-tauri/src/modules/voice/wake.rs` (l'écouteur alimente le collecteur) ;
  - `src-tauri/src/modules/voice/pipeline.rs` (`Conf` gagne `learning: bool` ; `Input` gagne `ExpectWake` et `FalseWake`) ;
  - `crates/core/src/settings.rs` (`voice_wake_learning: bool`, défaut `false`).
- Test: `learn.rs` (module `tests`, horloge et dossier injectés) et `wake.rs`.

**Interfaces:**
- Consumes : l'écouteur de la Tâche 4 (`WAKE_THRESHOLD`, `NEAR_MISS`, la machine à états) et `Endpoint` (Tâche 3).
- Produces :
  - `pub enum Label { Pos, Neg }`
  - `pub enum Event { Score { at_ms: u64, score: f32 }, Woke { at_ms: u64 }, Command, NoSpeech, Cancelled, ManualStart { at_ms: u64 }, ExpectWake { at_ms: u64 }, FalseWake { at_ms: u64 } }`
  - `pub struct Collector` :
    - `pub fn new(dir: PathBuf) -> Self` ;
    - `pub fn push_audio(&mut self, samples_16k: &[f32])`, avec un anneau de 6 s, en mémoire ;
    - `pub fn on(&mut self, e: Event) -> Option<(Label, Vec<f32>)>` : rend l'extrait à écrire, la logique pure, testée sans disque ;
    - `pub fn save(&mut self, label: Label, clip: &[f32]) -> std::io::Result<PathBuf>` : WAV 16 kHz mono i16, `learn/{pos,neg}/<horodatage>.wav`, puis rotation.
  - Constantes :
    - `POS_BEFORE_MS = 1400` et `POS_AFTER_MS = 200` : un positif fait 1,6 s et **finit 0,2 s après le meilleur score**. openWakeWord tronque les clips de plus de 2 s par la fin, ce qui couperait le mot.
    - `NEG_MS = 3000` ;
    - `RETRY_WINDOW_MS = 5000` ;
    - `EXPECT_WINDOW_MS = 5000` ;
    - `FALSE_WAKE_WINDOW_MS = 10000` ;
    - `EXPECT_MIN_SCORE = 0.05` ;
    - `MAX_FILES = 2000`, `MAX_BYTES = 200 Mo`.

**Règles d'étiquetage** (une ligne = un test ; les boutons l'emportent sur tout le reste) :

| Situation | Extrait |
|---|---|
| `ExpectWake`, puis `Woke` dans la fenêtre | `Pos`, autour du score du réveil |
| `ExpectWake`, puis aucun réveil dans la fenêtre | `Pos`, autour du meilleur score de la fenêtre, s'il dépasse `EXPECT_MIN_SCORE`, sinon rien |
| `FalseWake` moins de 10 s après un `Woke` | `Neg` (3 s finissant au réveil), et aucun `Pos` pour ce réveil |
| `Woke` puis `Command`, sans bouton | `Pos`, autour du score du réveil |
| `Woke` puis `NoSpeech` ou `Cancelled`, sans bouton | `Neg` |
| Score ≥ `NEAR_MISS` et < `WAKE_THRESHOLD`, puis `Woke` ou `ManualStart` dans les 5 s | `Pos`, autour du presque-réveil (raté, puis l'utilisateur a répété) |
| Presque-réveil sans suite dans les 5 s | rien (on ne sait pas) |

- [ ] **Step 1 : écrire les tests qui échouent.** Une fonction par ligne du tableau, plus :
  - `positive_clip_is_1600ms_ending_200ms_after_peak()` ;
  - `nothing_written_when_learning_off()` (`wake.rs` : `Conf.learning=false`, réveil puis commande → le dossier reste vide) ;
  - `rotation_keeps_newest_2000()` (2 005 sauvegardes → 2 000 fichiers, les 5 plus anciens supprimés) ;
  - `expect_wake_silence_writes_nothing()` (Review Focus 6).
- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `cargo test -p oniris voice::learn`. Attendu : ÉCHEC à la compilation.
- [ ] **Step 3 : écrire `Collector`, puis le brancher**
  - L'écouteur crée le `Collector` seulement si `Conf.learning` est vrai, et le détruit (anneau compris) dès que ce réglage repasse à faux.
  - L'écriture sur disque se fait hors du thread audio, par `tokio::task::spawn_blocking`.
  - Correspondance des événements :
    - `ManualStart` = `Input::Start` ;
    - `Cancelled` = `Input::Escape` entre `Woke` et la fin de l'enregistrement ;
    - `Command` = `Endpoint::End` ou `TooLong`.
  - Journal : une ligne `info` par extrait (« extrait d'apprentissage gardé : positif »), sans chemin ni score.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** `cargo test -p oniris`. Attendu : PASS, avec les tests des Tâches 4 et 6 et ceux du plan 6.
- [ ] **Step 5 : commit.** `feat(voix): mode apprentissage du mot d'activation`

### Tâche 7 : Boutons de la pilule, réentraînement du modèle principal, documents

**Files:**
- Modify :
  - `src/pill/Pill.tsx`, `src/pill/pill.css`, `src/pill/Pill.test.tsx` ;
  - `src/modules/voice/VoiceSection.tsx`, `src/modules/voice/voice.test.tsx`, `src/modules/voice/api.ts` ;
  - `src/lib/settings.ts` ;
  - `src-tauri/src/modules/voice/pipeline.rs` (commandes) ;
  - `tools/wakeword/generate_clips.py`, `tools/wakeword/README.md` ;
  - `docs/legal/confidentialite.md` (v9, section « Mode apprentissage ») ;
  - `docs/verifications/plan-08-mot-activation.md`.
- Create: `tools/wakeword/retrain.sh`

**Interfaces:**
- Consumes : `Input::ExpectWake`, `Input::FalseWake` et `voice_wake_learning` (Tâche 6).
- Produces :
  - commandes Tauri `voice_expect_wake()`, `voice_false_wake()` et `voice_learn_clear() -> u32` (nombre de fichiers supprimés) ;
  - `voice_learn_count() -> (u32, u32)`, soit (positifs, négatifs).
  - `PhaseEvent` (`oniris://voice`) gagne `by_wake: bool` : vrai quand la phase `Listening` vient du mot d'activation, et c'est ce qui affiche « Faux réveil ».

- [ ] **Step 1 : écrire les tests qui échouent**

```tsx
// Pill.test.tsx
it("bouton « Je vais dire Iris » seulement en mode apprentissage", ...) // learning=false → absent ; true → présent ; clic → invoke("voice_expect_wake")
it("« Faux réveil » apparaît 10 s après un réveil par la voix", ...)   // phase Listening venue du mot → bouton ; clic → invoke("voice_false_wake") ; disparaît après 10 s (timers simulés)
// voice.test.tsx
it("la case apprentissage est grisée tant que le mot d'activation est désactivé", ...)
it("affiche le compte des extraits et les supprime", ...)            // « 12 positifs, 3 négatifs » ; clic + confirmation → invoke("voice_learn_clear")
```
```python
# generate_clips.py --self-test (asserts, dossier temporaire)
# recordings/learn/pos/a.wav → positive_train/own_learn_a_000.wav, jamais dans positive_test
# recordings/learn/neg/b.wav → negative_train/live_learn_b_00.wav
```

- [ ] **Step 2 : lancer les tests et vérifier qu'ils échouent.** `pnpm vitest run src/pill src/modules/voice`, puis `python tools/wakeword/generate_clips.py --self-test`. Attendu : ÉCHEC.
- [ ] **Step 3 : écrire le code**
  - **Pilule.** Les deux boutons ne s'affichent que si `voice_wake_learning` est actif. Ils sont petits, sans texte d'aide, avec un `aria-label` égal à leur libellé.
    - « Je vais dire Iris » est toujours visible en mode apprentissage. Après un clic, il passe 5 s en état armé (anneau animé), sans autre retour.
    - « Faux réveil » n'est visible que pendant les 10 s qui suivent un réveil par la voix.
  - **Réglages,** sous la case du mot d'activation :
    - une case « Mode apprentissage » ;
    - la note exacte : « Garde sur ton PC de courts extraits autour des réveils pour réentraîner « Dis Iris ». Rien n'est envoyé. Les boutons de la pilule permettent de corriger IRIS. » ;
    - le compte des extraits et le bouton « Supprimer les extraits », avec une confirmation.
  - **`generate_clips.py`.**
    - `add_own` prend aussi `learn/pos/*.wav`, toujours en entraînement, jamais en test : le test reste tes enregistrements guidés, comparables d'un entraînement à l'autre.
    - `add_hard_negatives` prend aussi `learn/neg/*.wav`.
    - Les noms en sortie sont préfixés `learn_`.
  - **`retrain.sh`** réentraîne seulement le modèle principal :
    ```bash
    MODELS=dis_iris FRESH=1 GRID=${GRID:-128:1000:300,128:800:250,128:600:200} exec bash "$(dirname "$0")/train_best.sh"
    ```
    Le README gagne une section « 3 bis. Réentraîner avec le mode apprentissage » : quand relancer (après une centaine de nouveaux extraits), la commande, puis la vérification des nouveaux scores avant de garder le modèle.
  - **`confidentialite.md` v9, section « Mode apprentissage » :**
    - ce qui est gardé (extraits de 1,6 à 3 s autour d'un réveil ou d'un presque-réveil) ;
    - où ils sont gardés ;
    - qu'ils peuvent contenir d'autres voix de la pièce ;
    - la limite (2 000 fichiers ou 200 Mo) ;
    - comment les supprimer ;
    - que rien n'est transcrit ni envoyé.
  - **Vérifications.** La liste gagne :
    - activer le mode apprentissage, puis : réveil réussi suivi d'une commande → +1 positif ;
    - clic sur « Je vais dire Iris » suivi de « Dis Iris » sans réveil → +1 positif ;
    - réveil à tort puis clic sur « Faux réveil » → +1 négatif ;
    - « Supprimer les extraits » vide le dossier.
- [ ] **Step 4 : lancer les tests et vérifier qu'ils passent.** Mêmes commandes, plus `pnpm lint` et `pnpm build`. Attendu : tout est vert.
- [ ] **Step 5 : commit.** `feat(voix): boutons d'apprentissage et réentraînement du modèle principal`

---

## Après la partie B

- Revue finale de toute la branche, puis fusion `--ff-only` et `graphify update .`, comme aux plans 5 à 7.
- **Nouveau build de l'installateur (1.1.0) seulement avec l'accord de l'utilisateur**, puis la liste de vérification du plan 8.
- **Boucle d'amélioration :** usage avec le mode apprentissage, `retrain.sh` après une centaine d'extraits, puis comparaison des mesures (`sweep_dis_iris.md`) avant de remplacer le modèle livré. Chaque remplacement passe par la Tâche 0 bis (fixture, `SHA256SUMS`, commit), puis un nouveau build avec accord. Le plan 8 est fini quand l'objectif de la Tâche 0 est atteint en usage.
