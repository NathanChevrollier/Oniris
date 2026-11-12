# Oniris

Oniris est un tableau de bord personnel pour Windows 11 : une petite pilule toujours présente et un panneau plein écran qui s'ouvre d'un raccourci. Il regroupe agenda et rappels, notes, minuteur, presse-papiers, recherche de fichiers, favoris, projets, mails Gmail, musique (Spotify), sondes de sites, et **IRIS**, une assistante qui réfléchit d'abord sur ton PC et qu'on peut commander à la voix. Tout est rangé en local. Application Tauri 2 (Rust + React).

## Prérequis

- Windows 11.
- Facultatif : [Ollama](https://ollama.com) pour qu'IRIS réfléchisse en local ; Claude Code comme second cerveau d'IRIS.
- Facultatif : un compte Google (Gmail, Agenda) et un compte Spotify, connectés avec tes propres applications. Sans eux, Oniris fonctionne sans rien contacter.

## Installation

Deux voies :

- Lancer l'installeur `Oniris_1.0.0_x64-setup.exe` : il est en français et s'installe pour ton compte, sans administrateur.
- Le construire toi-même : `pnpm install`, `pnpm build`, puis `pnpm tauri build` (il faut aussi `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` dans l'environnement : la mise à jour exige un installeur signé, la clé reste dans ton 1Password). L'installeur sort dans `target\release\bundle\nsis\`. Le build est long.

La désinstallation retire l'entrée de démarrage automatique et **garde** `%APPDATA%\Oniris` et `%LOCALAPPDATA%\Oniris`.

## Premier lancement

Un accueil guidé te fait choisir tes réglages et te demande, une seule fois, si Oniris doit démarrer avec Windows : rien n'est ajouté au démarrage sans ton accord. Un appui bref sur Alt+Espace ouvre ou ferme le panneau ; en le maintenant, tu parles à IRIS (la voix se télécharge sur ton clic). En option, le mot d'activation « Dis Iris » (désactivé par défaut) permet de lui parler sans les mains : réglage dans la section Voix.

## Données et confidentialité

Tes données restent sur ton PC, dans `%APPDATA%\Oniris` (la voix dans `%LOCALAPPDATA%\Oniris\voice`). Aucune statistique d'usage n'est envoyée. Détails complets : [docs/legal/confidentialite.md](docs/legal/confidentialite.md).

## Licence

Tous droits réservés © 2026 Nathan Chevrollier : le code est consultable, mais sa copie, sa modification, sa redistribution et son usage commercial sont interdits sans accord écrit (voir [LICENSE](LICENSE)). Composants tiers et licences : [NOTICE.md](NOTICE.md).

## Mesures

Le banc de performances (ouverture du panneau, RAM, CPU, VRAM, fuite sur 24 h, mise en page) est dans `tools/perf`, voir [tools/perf/README.md](tools/perf/README.md). La liste de vérification globale est dans [docs/verifications/plan-07-finitions.md](docs/verifications/plan-07-finitions.md).

## Développement

```powershell
pnpm install
pnpm test                      # Vitest
cargo test --workspace         # tests Rust
cargo clippy --workspace --all-targets -- -D warnings
pnpm lint
pnpm build                     # vérification TypeScript + build du frontend
```

Aucun secret personnel n'est nécessaire pour le développement : les tests utilisent de faux serveurs et de faux programmes.

## Sortir une version

Quand tu dis « sors la version X.Y.Z », le contrôleur :

1. passe la version dans les 4 fichiers (`package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `crates/core/Cargo.toml`) ;
2. écrit les notes dans `docs/releases/vX.Y.Z.md` ;
3. lance `pnpm check:version` ;
4. commite et pose l'étiquette `vX.Y.Z` ;
5. te montre le tout, puis pousse après ton accord.

L'étiquette déclenche `.github/workflows/release.yml` : contrôle des versions, tests, build, signature et publication avec `latest.json`. Le workflow échoue si les notes manquent. Les secrets `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` se règlent dans GitHub (Settings, Secrets). Test du script : `node --test tools/release/check-version.test.mjs`.
