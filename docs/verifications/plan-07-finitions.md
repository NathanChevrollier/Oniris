# Plan 7 : liste de vérification globale

Rien de ce qui suit n'a été lancé par l'assistant : pas de build d'installeur, pas de lancement de l'app, pas de mesure. Cette liste est à dérouler à la main, dans l'ordre, sur ton PC.

## 1. Construire l'installeur (sur ton accord)

Le build est long. À la racine du dépôt :

```powershell
pnpm build
pnpm tauri build
```

L'installeur sort dans `target\release\bundle\nsis\` (le dossier `target` du workspace est à la racine, pas dans `src-tauri`) : `Oniris_1.0.0_x64-setup.exe`.

- [ ] Les deux commandes se terminent sans erreur et le `.exe` existe.

## 2. Installer et premier lancement complet

- [ ] Lance l'installeur : il est en français, ne demande pas d'administrateur, s'installe pour ton compte.
- [ ] Au premier lancement, déroule tout l'accueil, dont la question du démarrage automatique.
- [ ] Consentement : vérifie dans `regedit` (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) que la valeur « Oniris » n'existe **qu'après** ta réponse « oui ». Avec « non », elle ne doit pas exister.
- [ ] Les journaux sont dans `%APPDATA%\Oniris\logs`.
- [ ] Sur un écran 1080p, le panneau défile sans rien cacher (pas de zoom, voir la spec).

## 3. Budgets

`budgets.mjs` appelle `perf_open_cycles`, qui n'existe qu'en build de debug ou avec la feature `perf` : l'app installée ne l'a pas. On mesure donc le build release de mesure décrit dans `tools/perf/README.md`. **Build long : à lancer sur ton accord.** À la racine du dépôt, puis (le panneau clignote 30 fois, ne touche à rien pendant environ 3 minutes ; ferme Claude Code et Claude Desktop avant ; quitte l'Oniris installé) :

```powershell
pnpm build
cargo build --release -p oniris --features perf,custom-protocol
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
.\target\release\oniris.exe
# puis, dans un autre terminal :
node tools/perf/budgets.mjs
```

Ferme ensuite ce build (zone de notification > Quitter) et relance l'Oniris installé pour la suite. Tableau attendu :

| Mesure | Budget | Verdict attendu |
|---|---|---|
| Ouverture p95 (ms) | < 100 | OK |
| CPU repos (%) | < 0,5 | OK |
| RAM privée (Mo) | < 200 | OK |
| VRAM voix/IA (Mo) | 0 | OK (ou NON MESURABLE) |
| Processus voix/IA | 0 | OK |

- [ ] Tous les verdicts sont OK. Sinon : plan 7b (feuille de route).
- [ ] Mesure aussi la latence de la recherche avec beaucoup de longues notes (titre en flou, mots cherchés dans tout le texte).
- [ ] Le compteur VRAM `Get-Counter` fonctionne sur ta machine, et la détection des processus `ollama` runner et `claude` aussi.

## 4. Fuite mémoire sur 24 h

Sur l'app **installée** (pas le build de mesure), laisse-la tourner (usage normal ou repos), puis :

```powershell
node tools/perf/soak.mjs --hours 24
```

- [ ] Verdict **OK** (pente de RAM d'au plus 1 Mo/h, première heure exclue). **FUITE** : plan 7b.

## 5. Désinstaller

- [ ] Ferme Oniris, puis désinstalle-le (Paramètres > Applications).
- [ ] La valeur « Oniris » a disparu de la clé `Run`.
- [ ] `%APPDATA%\Oniris` et `%LOCALAPPDATA%\Oniris` sont toujours là (voulu : voir `docs/legal/confidentialite.md`).

## 6. Listes encore à faire

Ces listes décrivent ce que les tests automatiques ne peuvent pas voir : à faire sur le vrai PC.

- [ ] [Plan 1 : fondations](plan-01-fondations.md)
- [ ] [Plan 2 : modules locaux](plan-02-modules-locaux.md)
- [ ] [Plan 3 : recherche, favoris, projets](plan-03-recherche-projets.md)
- [ ] [Plan 4 : comptes en ligne](plan-04-comptes-en-ligne.md)
- [ ] [Plan 5 : IRIS](plan-05-iris.md)
- [ ] [Plan 6 : voix](plan-06-voix.md)
