#!/usr/bin/env bash
# Réentraîne le modèle principal avec TOUTES tes données propres, en une commande (WSL, après setup.sh).
#   bash retrain_all.sh --check     vérifie l'environnement (≈ 10 s)
#   bash retrain_all.sh --dry-run   + collecte, nettoyage et rapport, sans entraîner (≈ 1 min)
#   bash retrain_all.sh             tout : nettoyage, clips, caractéristiques, variantes, garde-fou, promotion
# Variables : MODEL (dis_iris), GRID (largeur:poids:positifs,...), STEPS (60000), JOBS (2 variantes en même temps),
#   FRESH=1 (refaire clips et caractéristiques même si les données n'ont pas changé), NO_PROMOTE=1 (mesurer sans
#   remplacer le modèle livré), LONG="fichiers sans « Iris »" (chemins sans espaces, ajoutés aux négatifs de mesure).
# Reprise : relancer la même commande. Les caractéristiques ne sont refaites que si les données retenues changent,
# et les variantes déjà entraînées sont reprises.
set -euo pipefail

KIT="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$KIT/../.." && pwd)"
WW="${WW_HOME:-$HOME/oniris-wakeword}"
REC="${RECORDINGS:-/mnt/c/Users/$(cmd.exe /c 'echo %USERNAME%' 2>/dev/null | tr -d '\r')/oniris-wakeword/recordings}"
MODEL="${MODEL:-dis_iris}"
GRID="${GRID:-128:1000:300,128:1500:300,128:2500:300,128:800:250}"
STEPS="${STEPS:-60000}"
JOBS="${JOBS:-2}"
MODE="${1:-all}"
CLEAN="$WW/clean/$MODEL"
OUT="$WW/output"
SHIPPED="$REPO/src-tauri/resources/wake/$MODEL.onnx"
quiet() { grep -vE "[Pp]honeme|Warning|warn|set_audio_backend|torchvision|it/s\]|augment = |augmented_samples|>>>|^\s*$" || true; }
say() { echo "### $(date +%H:%M) $*"; }

check() {
  local bad=0
  ok() { echo "  ok   $*"; }
  ko() { echo "  NON  $*"; bad=1; }
  grep -qi microsoft /proc/version 2>/dev/null && ok "WSL" || ko "pas dans WSL"
  if [ -x "$WW/venv/bin/python" ]; then
    "$WW/venv/bin/python" -c "import openwakeword, torch, onnxruntime, soundfile, scipy, yaml" 2>/dev/null \
      && ok "venv Python $("$WW/venv/bin/python" -V 2>&1 | cut -d' ' -f2) et dépendances" || ko "venv incomplet : relance setup.sh"
  else ko "venv absent ($WW/venv) : lance setup.sh"; fi
  grep -q "convert_to_tflite is True" "$WW/openWakeWord/openwakeword/train.py" 2>/dev/null && ok "openWakeWord corrigé" || ko "openWakeWord absent ou non corrigé : setup.sh"
  for f in data/validation_set_features.npy data/openwakeword_features_ACAV100M_2000_hrs_16bit.npy \
           openWakeWord/openwakeword/resources/models/melspectrogram.onnx openWakeWord/openwakeword/resources/models/embedding_model.onnx; do
    [ -s "$WW/$f" ] && [ ! -f "$WW/$f.part" ] && ok "$f" || ko "$f manquant ou incomplet : setup.sh"
  done
  for d in background rir; do
    n=$(find "$WW/data/$d" -name '*.wav' 2>/dev/null | wc -l); [ "$n" -gt 0 ] && ok "data/$d : $n WAV" || ko "data/$d vide : setup.sh"
  done
  n=$(find "$WW/voices" -name '*.onnx' 2>/dev/null | wc -l); [ "$n" -gt 0 ] && ok "voix Piper : $n" || ko "aucune voix Piper : setup.sh"
  free=$(df -BG --output=avail "$WW" | tail -1 | tr -dc 0-9); [ "$free" -ge 10 ] && ok "disque libre : ${free} Go" || ko "disque libre : ${free} Go (10 Go au moins)"
  if [ -d "$REC" ]; then
    ok "enregistrements : $REC ($(find "$REC" -maxdepth 1 -name '*.wav' | wc -l) à la racine, $(find "$REC/learn" -name '*.wav' 2>/dev/null | wc -l) du mode apprentissage, $(find "$REC/false_triggers" "$REC/near_misses" -name '*.wav' 2>/dev/null | wc -l) de listen_live)"
  else ko "enregistrements introuvables : $REC (RECORDINGS=...)"; fi
  if [ -f "$SHIPPED" ]; then
    (cd "$(dirname "$SHIPPED")" && sha256sum -c --quiet SHA256SUMS >/dev/null 2>&1) && ok "modèle livré et SHA256SUMS cohérents" || ko "SHA256SUMS ne correspond pas aux modèles du dépôt"
  else echo "  --   pas de modèle livré ($SHIPPED) : le meilleur candidat sera promu"; fi
  echo "  --   $(nproc) cœurs, $(free -g | awk '/Mem/{print $2}') Go de mémoire, JOBS=$JOBS"
  return $bad
}

estimate() {
  local n=0 todo feats=0
  IFS=',' read -ra g <<< "$GRID"
  for v in "${g[@]}"; do IFS=: read -r l w p <<< "$v"; [ -f "$OUT/sweep/${MODEL}_L${l}_N${w}_P${p}.onnx" ] || n=$((n + 1)); done
  [ "$1" = redo ] && { feats=20; n=${#g[@]}; }
  todo=$(( (n + JOBS - 1) / JOBS ))
  say "Estimation : ≈ $((feats + todo * 12 + 2)) min (caractéristiques $feats, $n variantes à entraîner ≈ 10-12 min chacune, ${JOBS} à la fois, mesures ≈ 2 min)"
}

case "$MODE" in --check|--dry-run|all) ;; *) echo "Usage : retrain_all.sh [--check|--dry-run]"; exit 2 ;; esac
say "Vérification de l'environnement"
if ! check; then echo "Environnement incomplet : corrige les lignes NON ci-dessus."; exit 1; fi
[ "$MODE" = --check ] && exit 0

cd "$WW"
# shellcheck disable=SC1091
source venv/bin/activate
say "1/5 Collecte et nettoyage (tes fichiers ne sont que lus)"
python "$KIT/clean_data.py" --ww "$WW" --recordings "$REC" --model "$MODEL" --out "$CLEAN" \
  --score-model "$SHIPPED" 2>&1 | quiet
HASH="$(cat "$CLEAN/data_hash.txt")"
redo=keep
if [ -n "${FRESH:-}" ] || [ ! -f "$OUT/$MODEL/positive_features_train.npy" ] || [ "$HASH" != "$(cat "$OUT/$MODEL/.data_hash" 2>/dev/null)" ]; then redo=redo; fi

if [ "$MODE" = --dry-run ]; then
  cp "$CLEAN/nettoyage.md" "$OUT/rapport_${MODEL}_essai.md"
  [ $redo = redo ] && say "Données changées depuis le dernier entraînement : clips et caractéristiques seront refaits." \
                   || say "Données inchangées : reprise possible sans refaire les caractéristiques."
  estimate $redo
  say "Essai fini, rien n'a été entraîné. Rapport : $OUT/rapport_${MODEL}_essai.md ; quarantaine : $CLEAN/quarantaine/"
  exit 0
fi

estimate $redo
if [ $redo = redo ]; then
  say "2/5 Clips (seuls les manquants sont générés ; tes données propres sont ajoutées)"
  MODEL=$MODEL RECORDINGS="$CLEAN/recordings" bash "$KIT/train.sh" clips 2>&1 | quiet
  say "3/5 Augmentation et caractéristiques (≈ 20 min)"
  # Variantes et caractéristiques d'une série précédente : obsolètes avec de nouvelles données.
  rm -rf "$OUT/$MODEL"/*.npy "$OUT/$MODEL/.data_hash" "$OUT/sweep/${MODEL}"_*
  MODEL=$MODEL bash "$KIT/train.sh" features 2>&1 | quiet
  echo "$HASH" > "$OUT/$MODEL/.data_hash"
else
  say "2-3/5 Données inchangées : clips et caractéristiques repris (FRESH=1 pour tout refaire)"
fi

say "4/5 Variantes : $GRID ($JOBS à la fois, $STEPS pas)"
python "$KIT/sweep.py" --ww "$WW" --model "$MODEL" --grid "$GRID" --steps "$STEPS" --jobs "$JOBS" 2>&1 | quiet

say "5/5 Garde-fou : modèle livré contre candidats, même test"
# shellcheck disable=SC2086
python "$KIT/gate.py" --ww "$WW" --repo "$REPO" --model "$MODEL" --sweep-json "$OUT/sweep_$MODEL.json" \
  --clean-report "$CLEAN/nettoyage.json" $([ -n "${NO_PROMOTE:-}" ] || echo --promote) ${LONG:+--long $LONG} 2>&1 | quiet
say "FIN. Quarantaine à écouter : $CLEAN/quarantaine/ (garder.txt / exclure.txt dans $CLEAN)"
