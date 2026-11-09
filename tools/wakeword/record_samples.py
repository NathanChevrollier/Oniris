"""Enregistre ta voix pour l'entraînement (à lancer sous Windows, pas dans WSL : WSL n'a pas le micro).

  py -3.12 -m pip install sounddevice soundfile numpy
  py -3.12 tools\\wakeword\\record_samples.py            # 60 « Iris » + 5 min d'ambiance
  py -3.12 tools\\wakeword\\record_samples.py --only iris
  py -3.12 tools\\wakeword\\record_samples.py --only ambient --ambient-secs 600

Les fichiers vont dans %USERPROFILE%\\oniris-wakeword\\recordings (jamais dans le dépôt : c'est ta voix).
Supprime ce dossier quand l'entraînement est fini si tu ne veux pas les garder.
"""
import argparse
import os
import time
from pathlib import Path

import numpy as np
import sounddevice as sd
import soundfile as sf

SR = 16000
HINTS = [
    "voix normale",
    "voix normale",
    "un peu plus loin du micro (1 à 2 m)",
    "vite",
    "lentement",
    "comme une question",
    "voix basse, presque à mi-voix",
    "en tournant la tête",
    "fatigué, sans articuler",
    "un peu plus fort",
]


def record(secs: float) -> np.ndarray:
    x = sd.rec(int(secs * SR), samplerate=SR, channels=1, dtype="float32")
    sd.wait()
    return x[:, 0]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=Path, default=Path(os.environ["USERPROFILE"]) / "oniris-wakeword" / "recordings")
    ap.add_argument("--count", type=int, default=60)
    ap.add_argument("--ambient-secs", type=int, default=300)
    ap.add_argument("--only", choices=["iris", "ambient"])
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    print(f"Micro : {sd.query_devices(kind='input')['name']}")
    print(f"Dossier : {a.out}\n")

    if a.only in (None, "iris"):
        start = len(list(a.out.glob("iris_*.wav")))
        for i in range(start, start + a.count):
            input(f"[{i - start + 1}/{a.count}] Entrée, puis dis « Iris » ({HINTS[i % len(HINTS)]}) ")
            time.sleep(0.15)
            x = record(1.8)
            peak = float(np.abs(x).max())
            if peak < 0.02:
                print("  trop faible, rien d'enregistré ; recommence")
                continue
            sf.write(a.out / f"iris_{i:04d}.wav", x, SR, subtype="PCM_16")
            print(f"  ok (pic {peak:.2f})")

    if a.only in (None, "ambient"):
        input(f"\nAmbiance : {a.ambient_secs} s du bruit normal de ta pièce (PC allumé, musique ou jeu si tu veux, "
              "mais sans dire « Iris »). Entrée pour commencer ")
        x = record(a.ambient_secs)
        name = f"ambient_{time.strftime('%Y%m%d_%H%M%S')}.wav"
        sf.write(a.out / name, x, SR, subtype="PCM_16")
        print(f"  {name} enregistré")


if __name__ == "__main__":
    main()
