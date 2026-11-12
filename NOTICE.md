# Notices et composants tiers

Oniris est sous licence « Tous droits réservés » (voir [LICENSE](LICENSE)). Les composants tiers ci-dessous
restent soumis à leurs propres licences.

| Composant | Usage | Licence |
|---|---|---|
| [Tauri](https://tauri.app) 2 (`Cargo.lock` : 2.12.5) | Coquille de l'application | Apache-2.0 OU MIT |
| [tract](https://github.com/sonos/tract) (`tract-onnx` 0.23.10) | Exécution des modèles ONNX du mot d'activation | MIT OU Apache-2.0 |
| [openWakeWord](https://github.com/dscripka/openWakeWord) | Détection du mot d'activation (code, modèles `melspectrogram` et `embedding`) | Code : Apache-2.0. Modèles pré-entraînés : CC BY-NC-SA 4.0 (voir ci-dessous) |
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (b5454) et modèles `ggml` | Transcription, téléchargés sur ton clic | licence à confirmer (modèles : dépôt `ggerganov/whisper.cpp`) |
| [Piper](https://github.com/rhasspy/piper) (2023.11.14-2) | Synthèse vocale, téléchargée sur ton clic | licence à confirmer |
| Voix Piper `fr_FR-siwis-medium` (jeu de données `siwis`) | Voix d'IRIS, téléchargée sur ton clic | licence à confirmer (dépôt `rhasspy/piper-voices` v1.0.0) |

Les dépendances Rust (`Cargo.lock`) et JavaScript (`package.json`, `pnpm-lock.yaml`) gardent chacune leur licence
d'origine, lisible dans leur paquet.

## Modèle du mot d'activation

`dis_iris.onnx` dérive de caractéristiques ACAV100M et de modèles openWakeWord, publiés pour un usage non commercial.
Le README d'openWakeWord indique : « All of the included pre-trained models are licensed under the Creative Commons
Attribution-NonCommercial-ShareAlike 4.0 International license due to the inclusion of datasets with unknown or
restrictive licensing as part of the training data. »
([CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/)). Le code du dépôt openWakeWord est, lui, sous Apache-2.0.

Le modèle `dis_iris.onnx` est fourni pour un **usage personnel non commercial** uniquement.
