"""Écoute en direct avec le modèle entraîné, pendant ta vraie journée (Windows, après `train.sh fixture`).

  py -3.12 -m pip install openwakeword==0.6.0 sounddevice soundfile numpy
  py -3.12 tools\\wakeword\\listen_live.py            # vie normale : tu ne dis PAS « Iris »
  py -3.12 tools\\wakeword\\listen_live.py --test     # tu dis « Iris » quand tu veux, on mesure le rappel

Vie normale (le mode qui compte) : joue, écoute de la musique, parle en vocal, sans dire « Iris ».
Chaque déclenchement est donc un FAUX déclenchement : un bip, l'heure, le score. Les 3 secondes autour
sont gardées dans recordings\\false_triggers (et les quasi-déclenchements, score ≥ --save-from, dans
recordings\\near_misses) : train.sh clips les ajoute aux négatifs au prochain entraînement.
Ctrl+C arrête et affiche le bilan (faux déclenchements par heure), ajouté à recordings\\sessions.jsonl.

Rien ne part sur Internet. Seuls les extraits autour d'un (quasi-)déclenchement sont écrits sur le disque,
dans ton dossier ; ils peuvent contenir d'autres voix de la pièce : supprime-les quand tu as fini.
"""
import argparse
import collections
import json
import os
import queue
import time
from pathlib import Path

import numpy as np
import sounddevice as sd
import soundfile as sf

SR = 16000
CHUNK = 1280  # 80 ms
BEFORE, AFTER = int(2.5 * SR / CHUNK), int(0.5 * SR / CHUNK)
REFRACTORY = 2.0
REPO = Path(__file__).resolve().parents[2]


def main() -> None:
    from openwakeword.model import Model

    ap = argparse.ArgumentParser()
    ap.add_argument("--models", type=Path, default=REPO / "src-tauri" / "resources" / "wake")
    ap.add_argument("--threshold", type=float, default=0.5)
    ap.add_argument("--save-from", type=float, default=0.3, help="garde aussi les quasi-déclenchements dès ce score")
    ap.add_argument("--out", type=Path, default=Path(os.environ["USERPROFILE"]) / "oniris-wakeword" / "recordings")
    ap.add_argument("--test", action="store_true", help="tu dis « Iris » : mesure du rappel")
    a = ap.parse_args()

    model = Model(
        wakeword_models=[str(a.models / "iris.onnx")],
        inference_framework="onnx",
        melspec_model_path=str(a.models / "melspectrogram.onnx"),
        embedding_model_path=str(a.models / "embedding_model.onnx"),
    )
    for d in ("false_triggers", "near_misses"):
        (a.out / d).mkdir(parents=True, exist_ok=True)

    q: "queue.Queue[np.ndarray]" = queue.Queue()
    ring = collections.deque(maxlen=BEFORE + AFTER)
    pending = []  # [(chunks restants, dossier, score, horodatage)]
    hits, near, last_hit, start = 0, 0, 0.0, time.time()
    mode = "TEST : dis « Iris » quand tu veux" if a.test else "VIE NORMALE : ne dis pas « Iris »"
    print(f"Micro : {sd.query_devices(kind='input')['name']} — {mode} — seuil {a.threshold} — Ctrl+C pour finir\n")

    def callback(indata, frames, t, status):
        q.put(indata[:, 0].copy())

    try:
        with sd.InputStream(samplerate=SR, channels=1, dtype="int16", blocksize=CHUNK, callback=callback):
            while True:
                chunk = q.get()
                ring.append(chunk)
                score = float(model.predict(chunk)["iris"])
                now = time.time()
                if score >= a.save_from and now - last_hit >= REFRACTORY:
                    last_hit = now
                    stamp = time.strftime("%Y%m%d_%H%M%S")
                    if score >= a.threshold:
                        hits += 1
                        print(f"{time.strftime('%H:%M:%S')}  DÉCLENCHÉ  score {score:.2f}" + ("" if a.test else "  (faux)"))
                        try:
                            import winsound
                            winsound.Beep(880, 120)
                        except Exception:
                            pass
                        folder = "false_triggers"
                    else:
                        near += 1
                        print(f"{time.strftime('%H:%M:%S')}  presque    score {score:.2f}")
                        folder = "near_misses"
                    if not a.test:
                        pending.append([AFTER, folder, score, stamp])
                for p in pending:
                    p[0] -= 1
                for p in [p for p in pending if p[0] <= 0]:
                    pending.remove(p)
                    sf.write(a.out / p[1] / f"{p[1][:2]}_{p[3]}_{p[2]:.2f}.wav", np.concatenate(ring), SR, subtype="PCM_16")
    except KeyboardInterrupt:
        pass

    hours = (time.time() - start) / 3600
    print(f"\nDurée : {hours * 60:.0f} min")
    summary = {"date": time.strftime("%Y-%m-%d %H:%M"), "mode": "test" if a.test else "vie", "threshold": a.threshold,
               "hours": round(hours, 3), "triggers": hits, "near_misses": near}
    if a.test:
        said = int(input("Combien de fois as-tu dit « Iris » ? ") or 0)
        summary["said"] = said
        if said:
            print(f"Rappel : {min(hits, said)}/{said} = {100 * min(hits, said) / said:.0f} %  (objectif ≥ 90 %)")
    else:
        rate = hits / hours if hours else 0
        summary["per_hour"] = round(rate, 3)
        print(f"Faux déclenchements : {hits} → {rate:.2f} par heure  (objectif ≤ 0,1)")
        print(f"Extraits gardés pour le réentraînement : {hits} faux + {near} presque, dans {a.out}")
    with open(a.out / "sessions.jsonl", "a", encoding="utf-8") as f:
        f.write(json.dumps(summary, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
