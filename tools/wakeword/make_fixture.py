"""Copie les modèles dans le dépôt Oniris et écrit la fixture de référence des tests Rust.

- src-tauri/resources/wake/{melspectrogram,embedding_model,iris}.onnx + SHA256SUMS
- src-tauri/tests/fixtures/wake/sequence.wav : silence, un « Iris » SYNTHÉTIQUE, silence, un négatif, silence
  (jamais ta voix : la fixture est commitée)
- src-tauri/tests/fixtures/wake/scores.json : le score d'openWakeWord (Python) à chaque pas de 80 ms
  Le détecteur Rust doit retrouver ces scores à 0,02 près à partir du pas 25 (2 s : openWakeWord
  amorce ses tampons avec du bruit aléatoire, les premiers pas ne sont pas reproductibles).
"""
import argparse
import hashlib
import json
import random
import shutil
import subprocess
from pathlib import Path

import numpy as np
import soundfile as sf

SR = 16000
CHUNK = 1280


def main() -> None:
    from openwakeword.model import Model

    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True)
    ap.add_argument("--repo", type=Path, required=True)
    ap.add_argument("--seed", type=int, default=8)
    a = ap.parse_args()
    repo = a.repo.resolve()
    res = a.ww / "openWakeWord" / "openwakeword" / "resources" / "models"
    model_path = a.ww / "output" / "iris.onnx"

    dst = repo / "src-tauri" / "resources" / "wake"
    dst.mkdir(parents=True, exist_ok=True)
    sums = []
    for src in (res / "melspectrogram.onnx", res / "embedding_model.onnx", model_path):
        shutil.copy(src, dst / src.name)
        sums.append(f"{hashlib.sha256((dst / src.name).read_bytes()).hexdigest()}  {src.name}")
    (dst / "SHA256SUMS").write_text("\n".join(sums) + "\n", encoding="utf-8")

    rng = random.Random(a.seed)
    test = a.ww / "output" / "iris"
    positives = sorted((test / "positive_test").glob("pos_*.wav"))
    rng.shuffle(positives)
    neg = rng.choice(sorted((test / "negative_test").glob("neg_*.wav")))
    sil = lambda s: np.zeros(int(s * SR), dtype=np.int16)
    read = lambda p: sf.read(p, dtype="int16")[0]
    model = Model(wakeword_models=[str(model_path)], inference_framework="onnx")
    # La fixture doit contenir une vraie détection : on prend le premier positif qui dépasse 0,7.
    for pos in positives[:100]:
        pcm = np.concatenate([sil(2.5), read(pos), sil(1.5), read(neg), sil(1.0)])
        pcm = pcm[: len(pcm) // CHUNK * CHUNK]
        model.reset()
        scores = [float(model.predict(pcm[i : i + CHUNK])["iris"]) for i in range(0, len(pcm), CHUNK)]
        if max(scores) >= 0.7:
            break
    else:
        raise SystemExit("Aucun positif de test ne dépasse 0,7 : le modèle n'est pas prêt.")

    fx = repo / "src-tauri" / "tests" / "fixtures" / "wake"
    fx.mkdir(parents=True, exist_ok=True)
    sf.write(fx / "sequence.wav", pcm, SR, subtype="PCM_16")
    oww = subprocess.run(["git", "-C", str(a.ww / "openWakeWord"), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    (fx / "scores.json").write_text(
        json.dumps({"chunk": CHUNK, "sample_rate": SR, "openwakeword": oww, "positive": pos.name,
                    "negative": neg.name, "compare_from": 25, "max": max(scores), "scores": scores}, indent=1),
        encoding="utf-8",
    )
    print("\n".join(sums))
    print(f"Fixture : {len(scores)} pas, score max {max(scores):.3f} ({pos.name} + {neg.name})")


if __name__ == "__main__":
    main()
