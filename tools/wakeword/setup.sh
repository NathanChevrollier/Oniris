#!/usr/bin/env bash
# Prépare l'entraînement du mot d'activation « Iris » dans WSL Ubuntu.
# Idempotent : relançable après une coupure (les téléchargements reprennent).
# Tout va dans $WW_HOME (défaut ~/oniris-wakeword, sur le disque ext4 de WSL : bien plus rapide que /mnt/c).
set -euo pipefail

KIT="$(cd "$(dirname "$0")" && pwd)"
WW="${WW_HOME:-$HOME/oniris-wakeword}"
HF="https://huggingface.co"
OWW_RES="https://github.com/dscripka/openWakeWord/releases/download/v0.5.1"
VOICES="$HF/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR"

mkdir -p "$WW"/{voices,data,output}
cd "$WW"

# Télécharge $1 vers $2 avec reprise ; ne refait rien si le fichier est complet.
fetch() {
  if [ -s "$2" ] && [ ! -f "$2.part" ]; then return; fi
  echo "→ $(basename "$2")"
  touch "$2.part"
  curl -L --fail --retry 5 -C - -o "$2" "$1"
  rm -f "$2.part"
}

echo "== Outils système"
for bin in git curl; do
  command -v "$bin" >/dev/null || { echo "Installe $bin : sudo apt-get install -y $bin"; exit 1; }
done
if ! command -v uv >/dev/null; then
  curl -LsSf https://astral.sh/uv/install.sh | sh
fi
export PATH="$HOME/.local/bin:$PATH"

echo "== Python 3.10 et dépendances"
[ -d venv ] || uv venv -p 3.10 venv
# shellcheck disable=SC1091
source venv/bin/activate
uv pip install torch==2.1.2 torchaudio==2.1.2 --index-url https://download.pytorch.org/whl/cpu
uv pip install -r "$KIT/requirements.txt"

echo "== openWakeWord et piper-sample-generator"
[ -d openWakeWord ] || git clone https://github.com/dscripka/openWakeWord
[ -d piper-sample-generator ] || git clone https://github.com/rhasspy/piper-sample-generator
uv pip install -e ./openWakeWord --no-deps
# train.py lit ses drapeaux comme des chaînes : sans ce correctif il tente la conversion TFLite
# (TensorFlow non installé) après chaque entraînement.
sed -i 's/if args.convert_to_tflite:/if args.convert_to_tflite is True:/' openWakeWord/openwakeword/train.py
{
  echo "openWakeWord $(git -C openWakeWord rev-parse HEAD)"
  echo "piper-sample-generator $(git -C piper-sample-generator rev-parse HEAD)"
} > versions.txt

echo "== Modèles de base openWakeWord (mel + embedding)"
RES=openWakeWord/openwakeword/resources/models
mkdir -p "$RES"
for f in melspectrogram.onnx embedding_model.onnx; do fetch "$OWW_RES/$f" "$RES/$f"; done

echo "== Voix Piper françaises (diversité de locuteurs)"
for v in mls/medium/fr_FR-mls-medium upmc/medium/fr_FR-upmc-medium siwis/medium/fr_FR-siwis-medium \
         tom/medium/fr_FR-tom-medium gilles/low/fr_FR-gilles-low; do
  name="$(basename "$v")"
  fetch "$VOICES/$v.onnx" "voices/$name.onnx"
  fetch "$VOICES/$v.onnx.json" "voices/$name.onnx.json"
done

echo "== Données d'entraînement (≈ 18 Go, le plus long)"
fetch "$HF/datasets/davidscripka/openwakeword_features/resolve/main/validation_set_features.npy" data/validation_set_features.npy
fetch "$HF/datasets/davidscripka/openwakeword_features/resolve/main/openwakeword_features_ACAV100M_2000_hrs_16bit.npy" \
      data/openwakeword_features_ACAV100M_2000_hrs_16bit.npy
# AudioSet est publié en parquet depuis 2025 (l'ancien bal_train09.tar renvoie 404) : un fragment ≈ 700 Mo.
rm -f data/bal_train09.tar data/bal_train09.tar.part
mkdir -p data/audioset
fetch "$HF/datasets/agkphysics/AudioSet/resolve/main/data/bal_train/09.parquet" data/audioset/bal_train09.parquet
huggingface-cli download --repo-type dataset davidscripka/MIT_environmental_impulse_responses \
  --local-dir data/mit_rirs_raw --local-dir-use-symlinks False

echo "== Conversion des bruits de fond et réverbérations en WAV 16 kHz"
python "$KIT/prepare_background.py" --ww "$WW"

df -h "$WW" | tail -1
echo "Prêt. Étape suivante : bash $KIT/train.sh"
