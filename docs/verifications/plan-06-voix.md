# Plan 6 : vérifications à faire à la main

La voix (micro, Whisper, Piper, haut-parleur) parle à du vrai matériel et à de vrais programmes. Aucun test n'a touché à ton micro, à tes haut-parleurs ni aux vrais `whisper-server.exe` et `piper.exe` : tout a été vérifié avec de faux programmes et de faux serveurs. Cette liste est la première vraie rencontre. Compte environ 1 h, plus le téléchargement (environ 280 Mo avec le modèle rapide, 665 Mo avec le précis).

Les cases marquées « ⚠️ à vérifier en priorité » sont les plus risquées. Whisper tourne sur le **processeur** (pas de Vulkan) : la mesure de la section 7 dit si c'est assez rapide.

## Lancer l'app en mode développement

Dans `Projets/Oniris` :

```powershell
pnpm tauri dev
```

Maintiens Alt+Espace pour parler ; un appui bref ouvre ou ferme le panneau. Les fichiers de la voix sont dans `%LOCALAPPDATA%\Oniris\voice` et les logs dans `%APPDATA%\Oniris\logs`. Ollama doit tourner (voir [`plan-05-iris.md`](plan-05-iris.md)).

## 1. Installer la voix

- [ ] ⚠️ à vérifier en priorité : ouvre la feuille IRIS (réglages). La section « Voix » affiche « À installer · N Mo » et un bouton « Installer la voix ».
- [ ] Clique sur « Installer la voix » : une barre de progression et « nom du fichier : N % » défilent. Ferme le panneau pendant le téléchargement : il continue.
- [ ] À la fin, l'état passe à « Installée ». `%LOCALAPPDATA%\Oniris\voice` contient `whisper`, `piper` et `models`.
- [ ] Coupe le réseau en plein téléchargement : une erreur s'affiche dans la section, sans plantage, et « Installer la voix » permet de reprendre.
- [ ] Choisis « Précis (turbo, 574 Mo) » dans « Transcription », « Enregistrer la voix » : l'état redevient « À installer » pour le modèle manquant ; réinstalle.

## 2. Une commande courte en jeu plein écran

- [ ] ⚠️ à vérifier en priorité : lance un jeu ou une vidéo en plein écran, joue de la musique (Spotify), maintiens Alt+Espace, dis « suivant », relâche. Le morceau change, **sans que le panneau s'ouvre**, et un court message s'affiche dans la pilule (elle s'agrandit pour le message, puis reprend sa taille au bout de 3 s).
- [ ] Pendant le maintien, la sphère de la pilule s'anime au rythme de ta voix.
- [ ] Dans le gestionnaire de tâches, `whisper-server.exe` s'arrête en plein écran peu après la fin de la commande (il ne reste pas chargé dans le jeu).

## 3. Une question longue

- [ ] ⚠️ à vérifier en priorité : hors plein écran, maintiens Alt+Espace et pose une question ouverte (« Qu'est-ce que j'ai demain ? »). Le panneau s'ouvre, ta phrase transcrite et la réponse d'IRIS s'affichent comme une conversation écrite, et IRIS la lit à voix haute.
- [ ] Le volume suit le curseur « Volume » des réglages ; décocher « IRIS répond à voix haute » puis « Enregistrer la voix » : la réponse reste écrite, sans son.

## 4. Interrompre IRIS

- [ ] Pendant qu'IRIS parle, appuie sur Échap : elle se tait aussitôt.
- [ ] Pendant qu'IRIS parle, appuie sur Alt+Espace (même bref) : elle se tait aussi, et le maintien lance bien une nouvelle écoute.

## 5. Micro débranché ou refusé

- [ ] Débranche ou désactive ton micro puis maintiens Alt+Espace : la pilule dit qu'aucun micro n'est disponible (ou que le micro n'a pas pu démarrer), sans plantage ni écoute bloquée.
- [ ] Choisis dans « Micro » un appareil qui n'existe plus (branche-le, enregistre, débranche-le) : l'écoute retombe sur le micro par défaut.
- [ ] Windows, Paramètres, Confidentialité, Microphone : refuse l'accès aux applications de bureau, puis maintiens le raccourci : le message demande d'autoriser le micro dans les paramètres de confidentialité de Windows. Remets l'autorisation après.
- [ ] Maintiens Alt+Espace sans parler ou plus de 30 s : le micro se ferme tout seul (le voyant de Windows s'éteint) et rien n'est exécuté en cas de silence.

## 6. Voix en pause et non installée

- [ ] Clic droit sur l'icône d'Oniris (zone de notification), coche « Mettre la voix en pause » : un maintien de Alt+Espace n'écoute plus (message « Voix en pause » dans la pilule), le micro reste fermé. Décoche : l'écoute revient.
- [ ] Supprime (Oniris fermé) le dossier `%LOCALAPPDATA%\Oniris\voice`, relance, maintiens le raccourci : message « Voix non installée ». Réinstalle ensuite.

## 7. Mesurer la latence (budget : moins de 1 s, modèles chargés)

Whisper tourne sur le processeur : si la mesure dépasse 1 s même avec `small`, note-le (l'accélération Vulkan est prévue au plan 7).

- [ ] Enregistre « mets la musique en pause » en WAV **16 kHz, mono, 16 bits** (par exemple avec Audacity : projet à 16000 Hz, piste mono, Exporter en WAV 16 bits), par exemple `C:\Temp\pause.wav`.
- [ ] Lance Whisper à la main (ce test ne démarre pas lui-même le serveur) :

  ```powershell
  & "$env:LOCALAPPDATA\Oniris\voice\whisper\whisper-server.exe" --host 127.0.0.1 --port 8178 -m "$env:LOCALAPPDATA\Oniris\voice\models\ggml-small-q5_1.bin" -l fr -t 8 --no-timestamps
  ```

- [ ] Dans un autre terminal, avec Ollama lancé :

  ```powershell
  $env:ONIRIS_TEST_WAV="C:\Temp\pause.wav"
  cargo test -p oniris-core voix_latence_reelle -- --ignored --nocapture
  ```

  Le test fait un tour de chauffe, puis mesure transcription et appel d'outil (`music__toggle`). Note la durée affichée et si l'outil a bien été appelé.
- [ ] Compare `small` et `turbo` : arrête Whisper, relance-le avec `-m "…\models\ggml-large-v3-turbo-q5_0.bin"` (installe-le avant, section 1), relance la commande. Garde `small` si la différence de qualité ne justifie pas le temps.
- [ ] Variables facultatives : `ONIRIS_TEST_WHISPER` (autre adresse du serveur) et `ONIRIS_TEST_MODEL` (autre modèle Ollama).
- [ ] Arrête le `whisper-server.exe` lancé à la main (Ctrl+C).

## 8. Repos : CPU à 0 et aucun processus de voix

- [ ] Panneau fermé, voix inutilisée depuis plus que « Libération (minutes) » : dans le gestionnaire de tâches, ni `whisper-server.exe` ni `piper.exe` ne tournent, et Oniris est à 0 % de processeur.
- [ ] Le voyant d'utilisation du micro de Windows est éteint entre deux usages.
- [ ] Après une commande vocale, `whisper-server.exe` disparaît au bout du délai réglé (« Libération (minutes) », 1 à 30).
- [ ] Ferme Oniris : plus aucun `whisper-server.exe` ni `piper.exe` ne reste.

## 9. Divers

- [ ] La case « Mot d'activation : bientôt » est grisée et non cochable (prévu au plan 8, optionnel).
- [ ] Les logs de `%APPDATA%\Oniris\logs` ne contiennent ni texte transcrit, ni texte lu par IRIS, ni chemin de ton dossier utilisateur.
