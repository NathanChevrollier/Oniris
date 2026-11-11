#!/usr/bin/env bash
# Grand entraînement : pour chaque modèle, clips complétés, caractéristiques recalculées, 5 variantes de réglages entraînées et
# mesurées de la même façon (sweep.py), la meilleure copiée dans Oniris. Environ 2 à 3 h, sans surveillance.
#   bash train_best.sh                      « Dis Iris » / « Hé Iris », puis « Iris » seul
#   MODELS=dis_iris bash train_best.sh      un seul modèle
set -euo pipefail

KIT="$(cd "$(dirname "$0")" && pwd)"
WW="${WW_HOME:-$HOME/oniris-wakeword}"
cd "$WW"
# shellcheck disable=SC1091
source venv/bin/activate
quiet() { grep -vE "[Pp]honeme|Warning|warn|set_audio_backend|torchvision|it/s\]|augment = |augmented_samples|>>>|^\s*$" || true; }

for m in ${MODELS:-dis_iris iris}; do
  echo "### $(date +%H:%M) $m : clips (seuls les manquants ou défectueux sont refaits)"
  MODEL=$m bash "$KIT/train.sh" clips 2>&1 | quiet
  echo "### $(date +%H:%M) $m : caractéristiques (≈ 6 min)"
  # Variantes et caractéristiques d'une série précédente : sweep.py réutiliserait ses modèles au lieu de réentraîner.
  rm -f "output/$m"/*.npy output/sweep/"$m"_*
  MODEL=$m bash "$KIT/train.sh" features 2>&1 | quiet
  echo "### $(date +%H:%M) $m : 5 variantes (≈ 10 min chacune)"
  python "$KIT/sweep.py" --ww "$WW" --model "$m" 2>&1 | quiet
  echo "### $(date +%H:%M) $m : copie dans Oniris"
  MODEL=$m bash "$KIT/train.sh" fixture 2>&1 | quiet
done
echo "### $(date +%H:%M) FIN — résultats : $WW/output/sweep_*.md"
