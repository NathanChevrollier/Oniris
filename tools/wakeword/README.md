# Entraîner le mot d'activation « Iris »

Ce kit produit `iris.onnx`, un petit modèle openWakeWord qui reconnaît « Iris » en français. Il est la partie A du plan 8 ([plan](../../docs/superpowers/plans/2026-11-09-oniris-08-mot-activation.md)). Oniris l'utilise ensuite dans la partie B.

- **Où :** WSL Ubuntu, déjà installé. Le calcul tourne sur le processeur : ta Radeon n'a pas CUDA, et le modèle est petit.
- **Disque :** environ 25 Go dans `~/oniris-wakeword`, sur le disque de WSL. Ils se libèrent à la fin (voir « Nettoyage »).
- **Temps :** compte environ une demi-journée, presque entièrement sans surveillance.
  - Ta voix : 15 min.
  - Téléchargements : 1 à 2 h selon la connexion.
  - Clips : 1 h.
  - Caractéristiques : 1 h.
  - Entraînement : 30 à 90 min.
  - Évaluation : 15 min.
- **Coût :** 0 €. Pas de compte, pas de clé.

## Pas à pas

### 0. Ta voix (Windows, 15 min)

C'est l'étape qui améliore le plus le modèle : les voix synthétiques ne suffisent pas à reconnaître la tienne.

```powershell
py -3.12 -m pip install sounddevice soundfile numpy
py -3.12 tools\wakeword\record_samples.py
```

Tu dis « Iris » 60 fois, avec des consignes variées : plus loin, vite, en question… Le script enregistre ensuite 5 min du bruit normal de ta pièce, PC allumé, musique ou jeu si tu veux.

Les fichiers vont dans `%USERPROFILE%\oniris-wakeword\recordings`, **jamais dans le dépôt**.

### 1. Installation et téléchargements (WSL, 1 à 2 h)

```bash
bash /mnt/c/Users/natha/Desktop/Utilitaires/Projets/Oniris/tools/wakeword/setup.sh
```

Si la connexion coupe, relance la même commande : les téléchargements reprennent.

Le script installe :
- Python 3.10, via `uv`, dans un venv à part ;
- openWakeWord et 5 voix Piper françaises, soit environ 130 locuteurs grâce à `fr_FR-mls` ;
- les données d'entraînement (environ 18 Go) :
  - caractéristiques ACAV100M (2 000 h d'audio varié, déjà converti en caractéristiques) ;
  - 11 h de validation ;
  - bruits AudioSet ;
  - réverbérations MIT.

Optionnel : tu peux déposer de la musique que tu écoutes souvent (.mp3, .flac, .wav) dans `~/oniris-wakeword/data/extra_src/`. Elle sert de bruit de fond.

### 2. Entraînement (WSL, environ 3 h sans surveillance)

```bash
bash /mnt/c/Users/natha/Desktop/Utilitaires/Projets/Oniris/tools/wakeword/train.sh
```

Le script enchaîne 5 étapes :
1. Clips français : 20 000 « Iris » et 20 000 négatifs (voir `negatives_fr.txt`), plus tes enregistrements.
2. Augmentation : bruit, réverbération, volume.
3. Entraînement : ça donne `~/oniris-wakeword/output/iris.onnx`.
4. Évaluation.
5. Copie des modèles et de la fixture dans le dépôt.

Pour relancer une seule étape : `train.sh clips|features|model|eval|fixture`.

### 2 bis. Grand entraînement : les meilleurs réglages (recommandé)

```bash
bash /mnt/c/Users/natha/Desktop/Utilitaires/Projets/Oniris/tools/wakeword/train_best.sh
```

Pour chaque modèle (« Dis Iris » / « Hé Iris », puis « Iris » seul), les clips manquants ou défectueux sont refaits, puis 5 variantes de réglages sont entraînées : largeur du réseau, poids des négatifs, positifs par lot. Elles sont toutes mesurées de la même façon :
- rappel sur 2 000 clips de test ;
- rappel sur **tes** enregistrements de test, joués en flux sur ton ambiance ;
- faux déclenchements par heure sur 11,3 h d'audio varié.

Pour chaque variante, le seuil retenu est le plus bas qui tient **≤ 0,5 faux déclenchement par heure**. La variante qui reconnaît le plus à ce seuil gagne et part dans Oniris. Compte 3 à 4 h, et garde un terminal WSL ouvert pendant ce temps. Le bilan est dans `~/oniris-wakeword/output/sweep_*.md`.

### 3. Écoute en direct (le plus important)

Le vrai test, c'est ta journée. Sous Windows, une fois `train.sh fixture` passé :

```powershell
py -3.12 -m pip install openwakeword==0.6.0
py -3.12 tools\wakeword\listen_live.py
```

1. **Vie normale, au moins 3 h au total, sur plusieurs sessions.** Laisse tourner pendant que tu joues, écoutes de la musique ou parles en vocal, **sans dire « Iris »**.
   - Chaque bip est un faux déclenchement.
   - Ctrl+C donne le bilan, en faux déclenchements par heure.
   - Les 3 s autour de chaque faux déclenchement et quasi-déclenchement sont gardées dans `recordings\false_triggers` et `recordings\near_misses`.
2. **Test de rappel, 5 min :** lance `listen_live.py --test`, dis « Iris » une vingtaine de fois, dans des conditions variées, puis Ctrl+C. Indique ensuite combien de fois tu l'as dit.
3. **S'il y a eu des faux déclenchements :** lance `bash .../train.sh retrain` dans WSL. Les extraits captés deviennent des négatifs, et le modèle est réentraîné en environ 2 h, puis recopié dans le dépôt. Refais ensuite une session de vie normale.

Les sessions s'ajoutent à `recordings\sessions.jsonl`. Les extraits peuvent contenir d'autres voix de ta pièce : ils restent sur ton PC, supprime-les une fois fini.

Optionnel : tu peux évaluer sur des fichiers plutôt qu'en direct (podcasts, vidéos), avec des chemins sans espaces :
`LONG="/mnt/c/.../podcast.mp3" bash .../train.sh eval`.

Objectif du plan 8 :
- **au moins 90 %** de tes « Iris » reconnus ;
- **au plus 0,1 faux déclenchement par heure**, soit moins d'un par soirée.

Le seuil par défaut est 0,5 (`--threshold`). `train.sh eval` en conseille un d'après les tests synthétiques et ta voix. Le seuil retenu devient la constante `WAKE_THRESHOLD` de la partie B.

### 3 bis. Réentraîner avec toutes les données (recommandé)

Quand : après une centaine de nouveaux extraits du mode apprentissage (compte dans les réglages d'Oniris, section « Voix »), une nouvelle prise guidée ou des sessions `listen_live.py`. Dans WSL :

```bash
K=/mnt/c/Users/natha/Desktop/Utilitaires/Projets/Oniris/tools/wakeword
bash $K/retrain_all.sh --check     # 10 s : WSL, venv, données téléchargées, disque, dossier Windows, SHA256SUMS
bash $K/retrain_all.sh --dry-run   # 1 min : collecte + nettoyage + rapport, sans entraîner
bash $K/retrain_all.sh             # 1 à 1 h 30 sans surveillance (estimation affichée au départ)
```

Relancer la même commande reprend après une interruption : les caractéristiques ne sont refaites que si les données retenues ont changé (`FRESH=1` pour forcer), et les variantes déjà entraînées sont gardées.

Les étapes :
1. **Collecte** dans `%USERPROFILE%\oniris-wakeword\recordings` : prises guidées (`dis_*`, `he_*`), mode apprentissage (`learn/pos`, `learn/neg`), `false_triggers`, `near_misses`, ambiance. Tes musiques vont, comme avant, dans `~/oniris-wakeword/data/extra_src/`.
2. **Nettoyage** (`clean_data.py`). Tes fichiers ne sont que **lus** : rien n'y est modifié ni supprimé. Les fichiers retenus sont copiés en WAV 16 kHz mono dans `~/oniris-wakeword/clean/dis_iris/recordings`.
   - **Rejetés (ignorés) :** illisibles, trop courts ou trop longs, silencieux, saturés, doublons. Un doublon se repère à son contenu audio, pas à son nom.
   - **En quarantaine** (copiés dans `clean/dis_iris/quarantaine/<raison>/`, à écouter) :
     - un positif sans pic de parole, ou dont la parole dure trop longtemps ;
     - un positif appris que le modèle actuel ne voit pas du tout ;
     - un négatif qui déclenche le modèle **et** ressemble fortement à tes « Dis Iris » enregistrés : il contient sans doute vraiment le mot.
   - **Ta décision :** pour garder un fichier mis en quarantaine, mets son nom dans `clean/dis_iris/garder.txt`. Pour écarter un fichier, mets-le dans `exclure.txt`. Puis relance.
3. **Découpage figé** (`~/oniris-wakeword/split_dis_iris.json`, par empreinte audio).
   - Un fichier déjà classé ne change jamais de côté.
   - Au premier passage, le test du dernier entraînement est repris tel quel.
   - Une nouvelle prise guidée va **entière** en entraînement ou en test (20 % des prises vont en test).
   - Les positifs appris vont en entraînement seulement. Les négatifs deviennent des négatifs durs.
   - Ta voix pèse environ 10 % des positifs, contre 20 000 synthétiques. Bruit, réverbération et volume viennent de l'augmentation du kit.
4. **Entraînement :** clips manquants, caractéristiques, puis 4 variantes (`GRID`, 2 à la fois avec `JOBS=2`, 60 000 pas).
5. **Garde-fou** (`gate.py`). Le modèle livré et chaque variante sont mesurés **exactement pareil** : même test figé de ta voix (même mélange d'ambiance), mêmes clips synthétiques, mêmes 11,3 h de négatifs. Pour chaque objectif, 0,5 puis 0,1 faux déclenchement par heure, on prend le seuil le plus bas qui le tient. Une variante n'est promue que si, aux deux objectifs, elle respecte ces trois conditions :
   - elle tient l'objectif si le modèle livré le tient ;
   - elle ne fait pas plus de faux déclenchements ;
   - elle ne reconnaît pas moins ta voix.

   L'ancien modèle est d'abord sauvegardé dans `~/oniris-wakeword/output/backups/` (modèle, `SHA256SUMS`, fixture). Le gagnant est copié dans `src-tauri/resources/wake` avec `SHA256SUMS` et la fixture (`make_fixture.py`). Si la fixture échoue, l'ancien modèle est remis. Sinon, l'ancien modèle reste en place.

Le rapport est dans `~/oniris-wakeword/output/rapport_dis_iris_<date>.md`. Il donne :
- les données gardées, rejetées et en quarantaine ;
- les scores de l'ancien modèle et des variantes ;
- la décision et le seuil conseillé.

`WAKE_THRESHOLD` (`src-tauri/src/modules/voice/wake.rs`) n'est **jamais** modifié automatiquement : le rapport donne la ligne à changer.

Options :
- `NO_PROMOTE=1` : mesurer sans rien remplacer.
- `GRID=128:1000:300,128:2500:300` : autres variantes.
- `JOBS=1` : si la mémoire manque.
- `LONG="/mnt/c/.../soiree.wav"` : audio long sans « Iris », ajouté aux négatifs de mesure.

Tests des parties sans données lourdes : `py -3.12 tools\wakeword\test_retrain.py`.

L'ancien `retrain.sh` remplace le modèle sans garde-fou : préfère `retrain_all.sh`.

### 4. Passage à la partie B

Une fois `train.sh fixture` passé, ces fichiers sont créés dans le dépôt :
- `src-tauri/resources/wake/` : les 3 modèles et `SHA256SUMS` ;
- `src-tauri/tests/fixtures/wake/` : un clip synthétique et les scores de référence. Ta voix n'y est pas.

Dis-moi « partie B », avec le seuil conseillé : j'intègre le modèle dans Oniris.

## Si le modèle est mauvais

- **Il rate ta voix** (rappel sous 90 %) :
  1. Enregistre 60 « Iris » de plus, dans d'autres conditions : casque, autre pièce, soir.
  2. Supprime les positifs et les caractéristiques : `rm -rf ~/oniris-wakeword/output/iris/positive_* ~/oniris-wakeword/output/iris/*.npy`.
  3. Relance `train.sh`.
- **Il se déclenche tout seul** (plus de 0,1 par heure) :
  1. Note les mots qui le trompent et ajoute-les à `negatives_fr.txt`.
  2. Supprime les négatifs et les caractéristiques : `rm -rf ~/oniris-wakeword/output/iris/negative_* ~/oniris-wakeword/output/iris/*.npy`.
  3. Relance.
  4. Tu peux aussi monter `steps` à 100 000 et `max_negative_weight` à 3 000 dans `iris.yml`.
- **Toujours insuffisant :** un mot de deux syllabes est court, donc difficile. On peut passer à « Dis Iris » ou « Hé Iris ».
  1. Change `POSITIVE_TEXTS` dans `generate_clips.py` et `target_phrase` dans `iris.yml`.
  2. Réenregistre-toi.
  3. Supprime `~/oniris-wakeword/output/`, puis relance.

  Ce changement demande ton accord : la spec dit « IRIS ».

## Confidentialité et licences

- Tes enregistrements restent sur ton PC, dans `%USERPROFILE%\oniris-wakeword\recordings`. Ils ne vont ni dans le dépôt, ni dans le modèle commité, ni sur Internet. Le modèle n'en garde qu'une empreinte statistique. Supprime le dossier quand tu as fini.
- Les caractéristiques ACAV100M et les modèles de base openWakeWord sont publiés pour la recherche. Le modèle `iris.onnx` qui en résulte est réservé à **ton usage personnel**, pas à une distribution commerciale d'Oniris.
- Le code d'openWakeWord est sous licence Apache 2.0, les voix Piper sous leurs licences respectives (MIT ou CC).

## Nettoyage

Une fois Oniris validé avec le modèle :

```bash
rm -rf ~/oniris-wakeword/data ~/oniris-wakeword/output/iris
```

Ça libère environ 22 Go. Garde `output/iris.onnx`, `versions.txt` et `venv` si tu veux réentraîner. Sous Windows, tu peux aussi supprimer `%USERPROFILE%\oniris-wakeword\recordings`.
