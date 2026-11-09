"""Mesure le modèle entraîné : rappel sur les « Iris » de test, faux déclenchements sur le reste.

  python evaluate.py --ww ~/oniris-wakeword [--recordings DIR] [--long FICHIER_OU_DOSSIER ...]

--long : de longs enregistrements SANS « Iris » (podcasts, vidéos, ta musique, une soirée de jeu en vocal…),
en .wav/.flac/.mp3. C'est la mesure qui compte : combien de faux déclenchements par heure de vraie vie.
Résultat : tableau par seuil + output/eval.json.
"""
import argparse
import json
from math import gcd
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

SR = 16000
CHUNK = 1280  # 80 ms : le pas d'openWakeWord
THRESHOLDS = [0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]
REFRACTORY = int(2.0 * SR / CHUNK)  # 2 s sans redéclenchement, comme dans Oniris
AUDIO = {".wav", ".flac", ".mp3", ".ogg"}


def load(path: Path) -> np.ndarray:
    x, sr = sf.read(path, dtype="float32", always_2d=False)
    if x.ndim > 1:
        x = x.mean(axis=1)
    if sr != SR:
        g = gcd(sr, SR)
        x = resample_poly(x, SR // g, sr // g)
    return (np.clip(x, -1, 1) * 32767).astype(np.int16)


def scores(model, pcm: np.ndarray) -> np.ndarray:
    model.reset()
    out = []
    for i in range(0, len(pcm) - CHUNK + 1, CHUNK):
        out.append(model.predict(pcm[i : i + CHUNK])["iris"])
    return np.array(out)


def clip_max(model, path: Path) -> float:
    pad = np.zeros(SR, dtype=np.int16)
    return float(scores(model, np.concatenate([pad, load(path), pad])).max())


def activations(s: np.ndarray, t: float) -> int:
    n, last = 0, -REFRACTORY
    for i, v in enumerate(s):
        if v >= t and i - last >= REFRACTORY:
            n, last = n + 1, i
    return n


def files(paths) -> list:
    out = []
    for p in paths:
        p = Path(p)
        out += [f for f in sorted(p.rglob("*")) if f.suffix.lower() in AUDIO] if p.is_dir() else [p]
    return out


def main() -> None:
    from openwakeword.model import Model

    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True)
    ap.add_argument("--recordings", type=Path, default=None)
    ap.add_argument("--long", nargs="*", default=[])
    a = ap.parse_args()
    out = a.ww / "output"
    model = Model(wakeword_models=[str(out / "iris.onnx")], inference_framework="onnx")

    test = out / "iris"
    synth = [clip_max(model, f) for f in sorted((test / "positive_test").glob("pos_*.wav"))]
    own = [clip_max(model, f) for f in sorted((test / "positive_test").glob("own_*.wav"))]
    neg = [clip_max(model, f) for f in sorted((test / "negative_test").glob("*.wav"))]

    long_scores, hours = [], 0.0
    for f in files(a.long):
        pcm = load(f)
        hours += len(pcm) / SR / 3600
        long_scores.append(scores(model, pcm))
        print(f"  {f.name} : {len(pcm) / SR / 60:.0f} min")

    rows = []
    for t in THRESHOLDS:
        row = {
            "seuil": t,
            "rappel_synthetique": float(np.mean(np.array(synth) >= t)) if synth else None,
            "rappel_ta_voix": float(np.mean(np.array(own) >= t)) if own else None,
            "faux_sur_negatifs_proches": float(np.mean(np.array(neg) >= t)) if neg else None,
            "faux_par_heure": (sum(activations(s, t) for s in long_scores) / hours) if hours else None,
        }
        rows.append(row)

    fmt = lambda v, pct=True: "—" if v is None else (f"{100 * v:5.1f} %" if pct else f"{v:6.2f}")
    print(f"\n{len(synth)} positifs synthétiques, {len(own)} de ta voix, {len(neg)} négatifs proches, {hours:.1f} h d'audio long")
    print("seuil | rappel synth | rappel ta voix | faux (proches) | faux / heure")
    for r in rows:
        print(f" {r['seuil']:.1f}  |   {fmt(r['rappel_synthetique'])}   |    {fmt(r['rappel_ta_voix'])}     |"
              f"    {fmt(r['faux_sur_negatifs_proches'])}     |  {fmt(r['faux_par_heure'], False)}")
    # Objectif du plan 8 : ≥ 90 % sur ta voix et ≤ 0.1 faux déclenchement par heure de vraie vie.
    ok = [r for r in rows if (r["rappel_ta_voix"] or 0) >= 0.9 and r["faux_par_heure"] is not None and r["faux_par_heure"] <= 0.1]
    print(f"\nSeuil conseillé : {ok[0]['seuil']}" if ok else "\nAucun seuil n'atteint l'objectif (≥ 90 % ta voix, ≤ 0.1 faux/h) : voir README, « Si le modèle est mauvais ».")
    (out / "eval.json").write_text(json.dumps({"hours_long": hours, "rows": rows}, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
