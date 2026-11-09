#!/usr/bin/env bash
# Entraîne le modèle « Iris » (après setup.sh). Chaque étape saute ce qui est déjà fait.
#   bash train.sh                 tout
#   bash train.sh clips|features|model|eval|fixture   une seule étape
#   bash train.sh retrain         après l'écoute en direct (listen_live.py)
set -euo pipefail

KIT="$(cd "$(dirname "$0")" && pwd)"
WW="${WW_HOME:-$HOME/oniris-wakeword}"
REC="${RECORDINGS:-/mnt/c/Users/$(cmd.exe /c 'echo %USERNAME%' 2>/dev/null | tr -d '\r')/oniris-wakeword/recordings}"
STEP="${1:-all}"
cd "$WW"
# shellcheck disable=SC1091
source venv/bin/activate
TRAIN="python openWakeWord/openwakeword/train.py --training_config $KIT/iris.yml"
run() { [ "$STEP" = all ] || [ "$STEP" = "$1" ]; }

# retrain : après des sessions listen_live.py, ajoute leurs extraits aux négatifs et réentraîne.
if [ "$STEP" = retrain ]; then
  rm -f output/iris/*.npy output/iris.onnx
  STEP=all
fi

if run clips; then
  echo "== 1/5 Bruits de fond (avec ton ambiance si elle existe : $REC)"
  python "$KIT/prepare_background.py" --ww "$WW" --recordings "$REC"
  echo "== 1/5 Clips français (≈ 30-60 min)"
  python "$KIT/generate_clips.py" --ww "$WW" --recordings "$REC" --workers "$(( $(nproc) / 2 ))"
fi
if run features; then
  echo "== 2/5 Augmentation + caractéristiques (≈ 1 h)"
  $TRAIN --augment_clips
fi
if run model; then
  echo "== 3/5 Entraînement (≈ 30-90 min sur processeur)"
  $TRAIN --train_model
  ls -la output/iris.onnx
fi
if run eval; then
  echo "== 4/5 Évaluation"
  # LONG="/mnt/c/.../podcast.mp3 /mnt/c/.../soiree.wav" : audio long sans « Iris » (chemins sans espaces)
  # shellcheck disable=SC2086
  python "$KIT/evaluate.py" --ww "$WW" --recordings "$REC" --long ${LONG:-}
fi
if run fixture; then
  echo "== 5/5 Fichiers pour Oniris (modèles + fixture de test Rust)"
  python "$KIT/make_fixture.py" --ww "$WW" --repo "$KIT/../.."
fi
