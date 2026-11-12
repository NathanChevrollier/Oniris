# Oniris : versions publiées et mise à jour automatique

Date : 2026-11-12. Statut : conception, à valider par l'utilisateur.

## Intention

Aujourd'hui, une nouvelle version d'Oniris demande trois gestes manuels :
- un build local de 13 min sur le PC de l'utilisateur ;
- la copie de l'installateur ;
- la réinstallation à la main.

L'utilisateur veut deux choses :
- **Publier une version sans effort,** seulement quand il la valide. Une version n'est pas publiée à chaque modification : c'est lui qui décide.
- **Recevoir la mise à jour simplement :** l'app se met à jour toute seule, sans réinstallation manuelle, avec le moins de friction possible.

## Décisions de l'utilisateur

- **Le dépôt GitHub `NathanChevrollier/Oniris` est public.**
  - L'utilisateur accepte que le code soit lisible.
  - Il veut une protection juridique : une licence qui interdit la copie, la modification et la redistribution sans son accord.
- **Une version ne part que quand l'utilisateur valide une release,** jamais à chaque modification.
- **Règles existantes, toujours en vigueur :**
  - pas de push sans validation de l'utilisateur ;
  - commits signés via 1Password, au format Conventional Commits en français, sans co-signature Claude ;
  - pas de build local sans accord.

## Objectifs

1. **Publication :** l'utilisateur dit « sors une version X.Y.Z ». La version est préparée et poussée après son accord, puis GitHub fabrique, signe et publie l'installateur tout seul, sans build sur le PC.
2. **Mise à jour :** Oniris détecte la nouvelle version, la télécharge en arrière-plan, vérifie sa signature et l'installe sans que l'utilisateur ait à relancer un installateur.
3. **Ouverture du dépôt sans fuite :** avant le premier push, l'historique est audité, et rien de secret ni de trop personnel ne part.
4. **Licences :** le code est sous licence propriétaire, et le modèle du mot d'activation porte une mention à part.

## Hors objectifs

- Une version à chaque commit, ou des versions nocturnes.
- Plusieurs canaux (bêta, stable).
- La signature de code Windows (certificat Authenticode). SmartScreen continuera d'avertir à la **première** installation. Les mises à jour suivantes passent par le système de mise à jour intégré, pas par un téléchargement manuel.
- macOS et Linux.
- Le retour automatique à la version précédente.

## Anti-objectifs (ce serait un échec même si ça marche)

- **Une mise à jour qui interrompt l'utilisateur au mauvais moment :** IRIS en train de parler ou de traiter une demande, ou une fenêtre qui vole le focus en plein jeu.
- **Une clé ou un secret dans le dépôt public :** clé de signature des mises à jour, jeton, identifiants OAuth, chemins personnels sensibles.
- **Une mise à jour non signée ou détournable :** il faut toujours une vérification cryptographique avant l'installation.
- **Une perte de données ou de réglages à la mise à jour :** la base SQLite, les réglages, les fichiers voix et les extraits d'apprentissage doivent survivre.
- **Un processus de publication qui exige de l'utilisateur des manipulations techniques à chaque version.**

## Contraintes

- **Stack existante :** Tauri 2, NSIS installé par utilisateur (configuration du plan 7, `src-tauri/tests/nsis_config.rs`), pnpm et Rust (workspace avec `crates/core` et `src-tauri`), Windows 11 x64.
- **Système de mise à jour :** le plugin officiel `tauri-plugin-updater`. Ses mises à jour sont signées par une paire de clés propre à Tauri, distincte d'Authenticode.
  - La clé publique est dans `tauri.conf.json`.
  - La clé privée et son mot de passe sont dans les secrets GitHub du dépôt, **jamais** dans le dépôt.
  - L'utilisateur garde une copie de la clé privée dans 1Password : sans elle, plus aucune mise à jour n'est possible pour les installations existantes.
- **Version :** elle doit être identique dans `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` et `crates/core/Cargo.toml`.
- **Vie privée :** la vérification des mises à jour contacte GitHub, qui voit l'adresse IP. `docs/legal/confidentialite.md` doit le dire, et passe en version 10.
- **Coût :** 0 €. Les minutes GitHub Actions d'un dépôt public sont gratuites.

## Comment ça marche (décidé)

### Publication

1. **L'utilisateur demande « sors une version X.Y.Z ».** Le contrôleur :
   - passe la version partout ;
   - rédige les notes de version en français, courtes, à partir des commits depuis la version précédente ;
   - commite ;
   - crée l'étiquette `vX.Y.Z` ;
   - **montre le tout à l'utilisateur**, puis pousse le commit et l'étiquette après son accord.
2. **Une étiquette `v*` déclenche un workflow GitHub Actions** sur `windows-latest`, avec `tauri-apps/tauri-action`. Il fait :
   - l'installation de pnpm et Rust, avec cache ;
   - la vérification que l'étiquette correspond à la version des fichiers, sinon échec ;
   - les tests (`cargo test`, `pnpm vitest run`) ;
   - le build NSIS ;
   - la signature de mise à jour ;
   - la création de la release GitHub avec l'installateur, sa signature et `latest.json`, et les notes de version.
3. **Si le workflow échoue,** rien n'est publié.

### Mise à jour dans l'app

- **Point de mise à jour :** `https://github.com/NathanChevrollier/Oniris/releases/latest/download/latest.json`.
- **Vérification :** au démarrage, en différé de quelques minutes pour ne pas ralentir l'ouverture (< 100 ms imposés par la spec), puis toutes les 6 h.
- **Téléchargement :** si une version plus récente existe, le téléchargement se fait en arrière-plan, sans fenêtre, et la signature est vérifiée.
- **Installation :**
  - Une notification discrète dans la pilule indique « Mise à jour X.Y.Z prête », avec un bouton « Redémarrer ».
  - Sans clic, la mise à jour s'installe à la **prochaine fermeture** de l'app, ou au redémarrage du PC.
  - Jamais pendant qu'IRIS écoute, réfléchit ou parle, ni quand une app est en plein écran.
- **Mode d'installation NSIS :** passif, sans assistant, avec relance de l'app après installation.
- **Réglage :** « Mises à jour automatiques », activé par défaut.
  - Désactivé, l'app ne contacte plus GitHub pour les mises à jour.
  - Un bouton « Vérifier maintenant » reste disponible.
- **Panneau « À propos » ou réglages :** il affiche la version installée et les notes de la dernière mise à jour.
- **En cas d'échec** (réseau, signature invalide, installation impossible) : une ligne de journal, pas de toast répété, et un nouvel essai au cycle suivant.

### Ouverture du dépôt

- **Audit avant le premier push :**
  - recherche de secrets dans tout l'historique : clés, jetons, `client_secret`, fichiers `.env`, clés privées, mots de passe ;
  - recherche de données personnelles sensibles.
- **Le rapport est montré à l'utilisateur.** Si un secret est trouvé, l'historique est réécrit ou le secret est révoqué, au choix de l'utilisateur, **avant** tout push.
- **`.gitignore` vérifié :** `graphify-out/`, `.superpowers/`, `target/` et les fichiers d'environnement n'y sont pas suivis.
- **`LICENSE` :** « Tous droits réservés », en français et en anglais. Lecture permise, toute copie, modification, redistribution ou utilisation commerciale interdite sans accord écrit.
- **`NOTICE` ou section du README :**
  - les composants tiers et leurs licences (openWakeWord Apache 2.0, voix Piper, whisper.cpp, etc.) ;
  - le cas du modèle `dis_iris.onnx` : il dérive de données réservées à un usage non commercial (vérifier la licence exacte publiée par openWakeWord pour ses caractéristiques ACAV100M et ses modèles), donc il n'est distribué que pour un usage personnel non commercial.

## Précisions (choix du concepteur, après relecture)

- **Deux façons d'installer :**
  - **Bouton « Redémarrer » :** installation passive, puis relance d'Oniris.
  - **Fermeture de l'app :** installation passive **sans** relance. Le gestionnaire de sortie Rust lance l'installateur téléchargé.

  « Redémarrage du PC » veut dire simplement la prochaine fermeture : rien ne s'exécute pendant l'extinction. Comme `install()` du plugin tue et relance l'app, le « téléchargé maintenant, installé plus tard » est conçu à la main : le plugin sert à télécharger et à vérifier, et l'exécution est décidée par l'app.
- **Ce que l'utilisateur fait une seule fois :** les étapes initiales lui sont acceptables, mais aucune n'est demandée à chaque version.
  - Le dépôt public existe et il est vide.
  - Le contrôleur génère la paire de clés (`pnpm tauri signer generate`) et dépose la clé privée et son mot de passe dans les secrets GitHub via `gh secret set`, sans jamais les afficher ni les commiter.
  - L'utilisateur range une copie de la clé privée et du mot de passe dans 1Password. Le contrôleur lui dit où se trouve le fichier, puis le fichier local est supprimé une fois la copie confirmée.
  - Les permissions Actions du dépôt sont réglées en écriture pour les releases.
- **Première version avec mise à jour : 1.2.0.** La 1.1.0 installée n'a pas le système de mise à jour : l'utilisateur installera la 1.2.0 à la main **une seule fois**. Ensuite, tout est automatique.
- **Tests en CI :**
  - `src-tauri/tests/nsis_config.rs` lit la version depuis `tauri.conf.json`, au lieu de `1.0.0` écrit en dur.
  - Les tests qui ont besoin d'un micro, d'une voix téléchargée ou du bureau restent `#[ignore]`.
  - Le test instable `modules::search::tests::indexation_vraie_jusqu_a_la_fin_du_premier_parcours` est corrigé (course entre `indexing()` et `IndexStatus::Ready`) avant d'activer la CI : un test instable bloquerait des versions au hasard.
- **Calme de l'utilisateur :** on réutilise ce qui existe :
  - le drapeau « tour en cours » et les phases de la voix (`pipeline.rs`) ;
  - la détection du plein écran déjà utilisée par l'écoute du mot d'activation.

  Seule l'**installation** attend ce calme. Le téléchargement, lui, se fait en arrière-plan à faible priorité.
- **Licence :**
  - Le texte est rédigé sur mesure : « Tous droits réservés », en français et en anglais.
  - Les champs `license` de `package.json` et des `Cargo.toml` passent à `LicenseRef-Proprietary`.
- **Données personnelles trouvées par l'audit :**
  - Elles sont seulement signalées : un chemin ou un nom dans un document n'est pas un secret.
  - L'historique n'est réécrit que si l'utilisateur le choisit explicitement, après avoir vu le rapport, parce qu'une réécriture fait perdre la signature des commits existants.
  - Un secret trouvé, lui, est révoqué dans tous les cas.

## Laissé au constructeur

- Le découpage exact du workflow et le cache.
- Le composant UI de la notification, en suivant le style des toasts de la pilule.
- Le nom et la place du réglage, en suivant les réglages existants.
- Le format exact des notes de version.
- L'outil d'audit des secrets (gitleaks ou équivalent), sans en faire une dépendance du projet.
