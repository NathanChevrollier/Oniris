# Banc de mesure des performances

> **État : rien n'a encore été mesuré.** Le banc est écrit et son test unitaire (`percentile`) passe, mais aucune mesure n'a été lancée (le panneau plein écran aurait clignoté 30 fois sur le bureau en cours d'usage). Le framework (React) n'est donc **ni confirmé ni infirmé** à ce stade.

## Ce que mesure le banc
- **Ouverture** : `perf_open_cycles(30)` (Rust, `src-tauri/src/perf.rs`) ouvre le panneau 30 fois. Pour chaque cycle : ouverture, attente de `perf_frame` (le panneau l'appelle avec `Date.now()` au 2e `requestAnimationFrame` après l'événement d'ouverture ; délai max 2 s), fermeture, pause de 300 ms. Écart = `frame_at_ms − instant juste avant open_panel`. Un cycle sans réponse sous 2 s est un *timeout* : exclu des statistiques et compté à part.
- **Armement** : le panneau ne fait l'IPC `perf_frame` que si Rust a émis `oniris://perf-armed = true` (pendant `perf_open_cycles` seulement). En usage normal, aucun IPC.
- **RAM** : `PrivateMemorySize64` = octets privés *committed*, un peu au-dessus du « working set privé » du Gestionnaire des tâches : chiffre pessimiste. Somme pour `oniris.exe` + tous les `msedgewebview2.exe` descendants, après 60 s de repos panneau fermé.
- **CPU** : temps CPU cumulé de ces processus sur 60 s ÷ 60 s ÷ nombre de cœurs.
- `perf_open_cycles` / `perf_frame` n'existent qu'en build debug ou avec `--features perf`.

## Procédure (à lancer par vous, quand c'est commode)
Prévoir ~3 minutes sans toucher à la machine. **Le panneau plein écran va clignoter environ 30 fois** pendant la première phase, puis plus rien (repos 60 s + mesure CPU 60 s).

Terminal 1 (PowerShell, depuis la racine du dépôt) :
```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
pnpm tauri dev
```
Attendre que la pilule apparaisse, puis Terminal 2 :
```powershell
node tools/perf/measure.mjs
```
Résultat : un tableau dans la console et `tools/perf/results-<AAAA-MM-JJ>.json`.

Mesure release (optionnelle, plus représentative). **Build long : à lancer par vous.** Un `cargo build --release` nu n'embarque pas le frontend (l'exe chargerait `devUrl`, donc une page blanche) : il faut `pnpm build` puis la feature `custom-protocol` (définie dans `src-tauri/Cargo.toml`) :
```powershell
pnpm build
cargo build --release -p oniris --features perf,custom-protocol
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
.\target\release\oniris.exe
# puis, dans un autre terminal :
node tools/perf/measure.mjs
```
Fermer ensuite Oniris (zone de notification > Quitter). Note : le build dev inclut les outils de débogage et gonfle RAM/latence ; le chiffre release fait foi.

## Lire `results-<date>.json`
- Le tableau affiche `INVALIDE` si `open.samples` vaut 0 ou si `open.timeouts` > 3 (plus de 10 % de cycles perdus) : ne rien conclure dans ce cas.
- `open.samples` : cycles valides sur 30 ; `open.timeouts` : cycles perdus (idéalement 0 ; beaucoup = mesure non fiable).
- `open.p50_ms`, `open.p95_ms`, `open.max_ms` : latence d'ouverture en ms (nearest-rank).
- `ramMb` : mémoire privée totale (Mo) ; `processes` : nombre de processus comptés ; `cpuPct` : CPU moyen au repos en % de la machine ; `cores`.
- `budget` : rappel des seuils. Budgets : ouverture p95 < 100 ms, CPU < 0,5 %, RAM < 200 Mo.

## Décision sur le framework (spec §8, §9) — règle de l'étape 7
- Si l'ouverture p95 tient sous 100 ms et que la RAM reste sous 200 Mo, **React est confirmé** et on le note ici.
- Sinon, mesurer la part de React en comparant avec une page `panel.html` sans React qui contient le même DOM statique. Si l'écart dépasse 20 ms d'ouverture ou 30 Mo de RAM, **s'arrêter et présenter les chiffres à l'utilisateur** avec la proposition de passer à SolidJS, avant d'attaquer le plan 2.
- Pistes à mesurer côté WebView2 : `--js-flags=--max-old-space-size`, `MemoryUsageTargetLevel = Low` quand le panneau est fermé (ICoreWebView2_19), et flou limité aux surfaces visibles.

## Résultats consignés
_Aucun pour l'instant._

## Mise en page (`layout.mjs`)
Mesure le panneau dans Edge headless contre un serveur Vite (données factices, aucune fenêtre ni IPC réels) à 2560×1440 et 1920×1080 (échelle 1,25), plus 1536×864 à titre indicatif : grille sans `.more` ni débordement de carte.
```powershell
node node_modules/vite/bin/vite.js --port 5199 --strictPort   # autre terminal ; l'arrêter ensuite (taskkill /T)
node tools/perf/layout.mjs http://localhost:5199/panel.html --shots <dossier-de-captures>
```
Écrit `tools/perf/layout-<date>.json` ; code de sortie 1 si une taille requise ne tient pas.
