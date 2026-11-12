# Notices et composants tiers

Oniris est sous licence « Tous droits réservés » (voir [LICENSE](LICENSE)). Les composants tiers ci-dessous
restent soumis à leurs propres licences.

| Composant | Usage | Licence |
|---|---|---|
| [Tauri](https://tauri.app) 2 (`Cargo.lock` : 2.12.5) | Coquille de l'application | Apache-2.0 OU MIT |
| [tract](https://github.com/sonos/tract) (`tract-onnx` 0.23.10) | Exécution des modèles ONNX du mot d'activation | MIT OU Apache-2.0 |
| [openWakeWord](https://github.com/dscripka/openWakeWord) | Détection du mot d'activation (code, modèles `melspectrogram` et `embedding`) | Code : Apache-2.0. Modèles pré-entraînés : CC BY-NC-SA 4.0 (voir ci-dessous) |
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (b5454) et modèles `ggml` (`ggerganov/whisper.cpp`), téléchargés sur ton clic | Transcription | MIT, © The ggml authors ([licence du code](https://raw.githubusercontent.com/ggml-org/whisper.cpp/master/LICENSE)) ; modèles : MIT ([Hugging Face](https://huggingface.co/ggerganov/whisper.cpp)) |
| [Piper](https://github.com/rhasspy/piper) (2023.11.14-2), téléchargé sur ton clic | Synthèse vocale | MIT, © 2022 Michael Hansen ([licence](https://raw.githubusercontent.com/rhasspy/piper/master/LICENSE.md)) |
| Voix Piper `fr_FR-siwis-medium` (`rhasspy/piper-voices` v1.0.0), téléchargée sur ton clic | Voix d'IRIS | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Attribution : entraînée sur la [SIWIS French Speech Synthesis Database](https://datashare.is.ed.ac.uk/handle/10283/2353) (University of Edinburgh DataShare) ; [fiche du modèle](https://huggingface.co/rhasspy/piper-voices/raw/main/fr/fr_FR/siwis/medium/MODEL_CARD) |

Les dépendances Rust (`Cargo.lock`) et JavaScript (`package.json`, `pnpm-lock.yaml`) gardent chacune leur licence
d'origine, lisible dans leur paquet.

## Modèle du mot d'activation

`dis_iris.onnx` dérive de caractéristiques ACAV100M et de modèles openWakeWord, publiés pour un usage non commercial.
Le README d'openWakeWord indique : « All of the included pre-trained models are licensed under the Creative Commons
Attribution-NonCommercial-ShareAlike 4.0 International license due to the inclusion of datasets with unknown or
restrictive licensing as part of the training data. »
([CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/)). Le code du dépôt openWakeWord est, lui, sous Apache-2.0.

Le modèle `dis_iris.onnx` est fourni pour un **usage personnel non commercial** uniquement.
