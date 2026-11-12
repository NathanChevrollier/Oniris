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

## Modèles du mot d'activation (licence propre, hors LICENSE)

Les fichiers `dis_iris.onnx`, `melspectrogram.onnx` et `embedding_model.onnx` ne sont **pas** couverts par la licence
propriétaire d'Oniris (voir [LICENSE](LICENSE)). Ils restent distribués sous leur propre licence :
[Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International (CC BY-NC-SA 4.0)](https://creativecommons.org/licenses/by-nc-sa/4.0/).

- `melspectrogram.onnx` et `embedding_model.onnx` viennent du projet [openWakeWord](https://github.com/dscripka/openWakeWord) de David Scripka
  (l'extracteur d'embeddings dérive d'un modèle de Google sous Apache-2.0).
- `dis_iris.onnx` est un modèle entraîné pour Oniris à partir d'openWakeWord et de caractéristiques pré-calculées
  [ACAV100M](https://github.com/dscripka/openWakeWord#training-new-models) ; c'est une œuvre dérivée, partagée aux mêmes conditions (ShareAlike).

Le README d'openWakeWord indique : « All of the included pre-trained models are licensed under the Creative Commons
Attribution-NonCommercial-ShareAlike 4.0 International license due to the inclusion of datasets with unknown or
restrictive licensing as part of the training data. » Le code du dépôt openWakeWord est, lui, sous Apache-2.0.

Ces trois modèles sont fournis pour un **usage personnel non commercial** uniquement, avec attribution à openWakeWord et ACAV100M.
