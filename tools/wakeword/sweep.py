"""Recherche des meilleurs réglages d'un modèle du mot d'activation : entraîne plusieurs variantes sur les mêmes
caractéristiques, les mesure toutes de la même façon, garde la meilleure.

  python sweep.py --ww ~/oniris-wakeword --model dis_iris [--target-fph 0.5] [--steps 60000]

Mesures de chaque variante :
- rappel synthétique : caractéristiques de test (2 000 clips augmentés jamais vus à l'entraînement) ;
- rappel sur TA voix : tes enregistrements de test, joués en flux sur ton ambiance, comme dans Oniris ;
- faux déclenchements par heure : 11,3 h d'audio varié (validation d'openWakeWord), en flux, 2 s de pause
  après chaque déclenchement, comme dans Oniris.
Pour chaque variante, le seuil retenu est le plus bas qui tient l'objectif de faux déclenchements ; la meilleure
variante est celle qui reconnaît le plus à ce seuil (60 % synthétique, 40 % ta voix).
Résultat : output/<model>.onnx (la gagnante), output/sweep_<model>.md et .json.
Limite : la validation sert aussi à openWakeWord pour choisir ses points d'étape ; le taux mesuré est donc un
peu optimiste, d'où l'écoute en direct ensuite.
"""
import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import yaml

KIT = Path(__file__).resolve().parent
sys.path.insert(0, str(KIT))
import evaluate  # noqa: E402  (fond sonore et lecture en flux, partagés avec l'évaluation)

THRESHOLDS = [round(t, 2) for t in np.arange(0.2, 0.96, 0.05)]
VAL_HOURS = 11.3  # durée du jeu de validation d'openWakeWord (train.py)
REFRACTORY = 25  # pas de 80 ms : 2 s
# (largeur des couches, poids maximal des négatifs, positifs par lot). 1re série (11/11, 50 positifs par lot,
# poids 500 à 2000) : sous-apprentissage, 32 % de rappel même sur l'entraînement. Les lots comptaient 50 positifs
# pour plus de 1 000 négatifs, eux-mêmes surpondérés. D'où plus de positifs et des poids bien plus doux ; le seuil
# choisi ensuite tient les faux déclenchements.
GRID = [(64, 100, 300), (64, 300, 300), (128, 100, 300), (128, 300, 300), (128, 300, 150)]


def session(path: Path):
    import onnxruntime as ort

    s = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    return s, s.get_inputs()[0].name


def feature_scores(path: Path, x: np.ndarray) -> np.ndarray:
    s, name = session(path)
    return np.array([s.run(None, {name: x[i : i + 1].astype(np.float32)})[0].reshape(-1)[0] for i in range(len(x))])


def val_fph(path: Path, val: np.ndarray) -> dict:
    """Faux déclenchements par heure à chaque seuil, en flux sur la validation (fenêtres de 16, pas de 1)."""
    from numpy.lib.stride_tricks import sliding_window_view

    win = sliding_window_view(val, (16, val.shape[1]))[:, 0]
    s = feature_scores(path, win)
    out = {}
    for t in THRESHOLDS:
        n, last = 0, -REFRACTORY
        for i in np.flatnonzero(s >= t):
            if i - last >= REFRACTORY:
                n, last = n + 1, i
        out[t] = n / VAL_HOURS
    return out


def own_recall(path: Path, ww: Path, model: str) -> list:
    from openwakeword.model import Model

    files = sorted((ww / "output" / model / "positive_test").glob("own_*.wav"))
    if not files:
        return []
    m = Model(wakeword_models=[str(path)], inference_framework="onnx")
    evaluate.NAME = path.stem
    return [evaluate.clip_max(m, f) for f in files]


def train(ww: Path, base: dict, model: str, layer: int, neg: int, pos: int, steps: int, out: Path) -> Path:
    batch = dict(base["batch_n_per_class"], positive=pos, adversarial_negative=max(50, pos // 3))
    cfg = dict(base, layer_size=layer, max_negative_weight=neg, steps=steps, batch_n_per_class=batch)
    name = f"{model}_L{layer}_N{neg}_P{pos}"
    ypath = out / f"{name}.yml"
    ypath.write_text(yaml.safe_dump(cfg, allow_unicode=True), encoding="utf-8")
    log = out / f"{name}.log"
    with open(log, "w") as f:
        r = subprocess.run([sys.executable, "openWakeWord/openwakeword/train.py", "--training_config", str(ypath), "--train_model"],
                           cwd=ww, stdout=f, stderr=subprocess.STDOUT)
    if r.returncode != 0:
        raise RuntimeError(f"entraînement en échec, voir {log}")
    dst = out / f"{name}.onnx"
    shutil.move(str(ww / "output" / f"{model}.onnx"), dst)
    return dst


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True)
    ap.add_argument("--model", default="dis_iris")
    ap.add_argument("--target-fph", type=float, default=0.5)
    ap.add_argument("--steps", type=int, default=60000)
    a = ap.parse_args()
    out = a.ww / "output" / "sweep"
    out.mkdir(parents=True, exist_ok=True)
    base = yaml.safe_load((KIT / f"{a.model}.yml").read_text(encoding="utf-8"))
    feats = a.ww / "output" / a.model
    pos = np.load(feats / "positive_features_test.npy")
    val = np.load(a.ww / "data" / "validation_set_features.npy", mmap_mode="r")
    val = np.asarray(val.reshape(-1, val.shape[-1]) if val.ndim > 2 else val, dtype=np.float32)
    evaluate.load_bed(a.ww)

    results = []
    for layer, neg, posb in GRID:
        name = f"L{layer}_N{neg}_P{posb}"
        t0 = time.time()
        cand = out / f"{a.model}_{name}.onnx"
        if not cand.exists():
            print(f"[{time.strftime('%H:%M')}] {a.model} {name} : entraînement…", flush=True)
            cand = train(a.ww, base, a.model, layer, neg, posb, a.steps, out)
        synth = feature_scores(cand, pos)
        own = np.array(own_recall(cand, a.ww, a.model))
        fph = val_fph(cand, val)
        ok = [t for t in THRESHOLDS if fph[t] <= a.target_fph]
        t = ok[0] if ok else min(THRESHOLDS, key=lambda x: fph[x])
        r = {
            "variante": name, "seuil": t, "faux_par_heure": round(fph[t], 3),
            "rappel_synth": round(float(np.mean(synth >= t)), 3),
            "rappel_ta_voix": round(float(np.mean(own >= t)), 3) if len(own) else None,
            "objectif_tenu": bool(ok), "minutes": round((time.time() - t0) / 60, 1),
        }
        r["score"] = round(0.6 * r["rappel_synth"] + 0.4 * (r["rappel_ta_voix"] if r["rappel_ta_voix"] is not None else r["rappel_synth"]), 3)
        results.append(r)
        print(f"  {name} : seuil {t}, {r['faux_par_heure']} faux/h, rappel synth {r['rappel_synth']:.0%}, "
              f"ta voix {r['rappel_ta_voix'] if r['rappel_ta_voix'] is None else format(r['rappel_ta_voix'], '.0%')}, score {r['score']}", flush=True)

    best = max(results, key=lambda r: (r["objectif_tenu"], r["score"]))
    shutil.copy(out / f"{a.model}_{best['variante']}.onnx", a.ww / "output" / f"{a.model}.onnx")
    md = [f"# Recherche de réglages : {a.model}", "", f"Objectif : ≤ {a.target_fph} faux déclenchement par heure.", "",
          "| variante | seuil | faux/h | rappel synth | rappel ta voix | score | objectif |", "|---|---|---|---|---|---|---|"]
    for r in sorted(results, key=lambda r: -r["score"]):
        md.append(f"| {r['variante']}{' **(retenue)**' if r is best else ''} | {r['seuil']} | {r['faux_par_heure']} | {r['rappel_synth']:.0%} | "
                  f"{'—' if r['rappel_ta_voix'] is None else format(r['rappel_ta_voix'], '.0%')} | {r['score']} | {'oui' if r['objectif_tenu'] else 'non'} |")
    (a.ww / "output" / f"sweep_{a.model}.md").write_text("\n".join(md) + "\n", encoding="utf-8")
    (a.ww / "output" / f"sweep_{a.model}.json").write_text(json.dumps({"best": best, "all": results}, indent=1), encoding="utf-8")
    print("\n".join(md))
    print(f"\nRetenue : {best['variante']}, seuil {best['seuil']} → output/{a.model}.onnx")


if __name__ == "__main__":
    main()
