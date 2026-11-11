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
  # Reprise après interruption : caractéristiques déjà faites, on garde aussi les variantes déjà entraînées.
  # FRESH=1 force tout à refaire (après un changement des clips ou de leur génération).
  if [ -n "${FRESH:-}" ] || [ ! -f "output/$m/positive_features_train.npy" ]; then
    echo "### $(date +%H:%M) $m : clips (seuls les manquants ou défectueux sont refaits)"
    MODEL=$m bash "$KIT/train.sh" clips 2>&1 | quiet
    echo "### $(date +%H:%M) $m : caractéristiques (≈ 6 min)"
    # Variantes d'une série précédente : sweep.py les réutiliserait au lieu de réentraîner.
    rm -f "output/$m"/*.npy output/sweep/"$m"_*
    MODEL=$m bash "$KIT/train.sh" features 2>&1 | quiet
  else
    echo "### $(date +%H:%M) $m : reprise (clips et caractéristiques déjà faits, FRESH=1 pour tout refaire)"
  fi
  echo "### $(date +%H:%M) $m : 5 variantes (≈ 10 min chacune)"
  python "$KIT/sweep.py" --ww "$WW" --model "$m" 2>&1 | quiet
  echo "### $(date +%H:%M) $m : copie dans Oniris"
  MODEL=$m bash "$KIT/train.sh" fixture 2>&1 | quiet
done
echo "### $(date +%H:%M) FIN — résultats : $WW/output/sweep_*.md"
