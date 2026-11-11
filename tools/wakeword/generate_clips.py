"""Génère les clips d'entraînement en français avec les voix Piper, à la place de l'étape --generate_clips
d'openWakeWord (qui ne sait produire que de l'anglais).

Sortie, dans le dossier attendu par openwakeword/train.py :
  output/iris/positive_train  positive_test  negative_train  negative_test   (WAV mono 16 kHz 16 bits)

Tes propres enregistrements (record_samples.py) sont ajoutés aux positifs : 80 % en entraînement
(dupliqués pour peser ~10 % des positifs, l'augmentation les rend tous différents), 20 % en test.
Relançable : un clip déjà présent est sauté.
"""
import argparse
import json
import multiprocessing as mp
import random
import shutil
from math import gcd
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

SR = 16000
TARGET = "iris"
# Phrases positives, enregistrements personnels et durée maximale d'un positif (s, marges comprises) par modèle.
# Piper déraille parfois (mls surtout) et bredouille plusieurs secondes : 35 % des positifs du 11/11 dépassaient
# cette durée, jusqu'à 7 s pour « Dis Iris ». openWakeWord ne regarde que la fin du clip (1,28 s) : ces faux
# positifs lui apprenaient à se déclencher sur n'importe quelle parole. Tes enregistrements font au plus 1,2 et 1,3 s.
MODELS = {
    "iris": (["Iris", "Iris.", "Iris !", "Iris ?", "Iris,", "Iris…", "iris"], ["iris_*.wav"], 1.2),
    "dis_iris": (["Dis Iris", "Dis, Iris", "Dis Iris !", "Dis Iris ?", "Hé Iris", "Hé, Iris", "Hé Iris !", "Hé Iris ?"],
                 ["dis_*.wav", "he_*.wav"], 1.6),
}
TRIES = 4  # nouveaux tirages d'un positif trop long avant d'y renoncer

_voices = []  # (PiperVoice, nb de locuteurs), chargées une fois par processus


def load_voices(voice_dir: str) -> None:
    from piper import PiperVoice

    for p in sorted(Path(voice_dir).glob("*.onnx")):
        v = PiperVoice.load(str(p))
        _voices.append((v, max(1, int(v.config.num_speakers or 1))))


def speakers_of(voice_dir: Path) -> list:
    """Locuteurs par voix, lus dans les .onnx.json : le processus principal ne charge jamais onnxruntime
    (une session ONNX dupliquée par fork bloque les workers)."""
    return [max(1, int(json.loads(Path(f"{p}.json").read_text(encoding="utf-8")).get("num_speakers") or 1))
            for p in sorted(voice_dir.glob("*.onnx"))]


def read_negatives(path: Path) -> list:
    out = [l.strip() for l in path.read_text(encoding="utf-8").splitlines()]
    out = [l for l in out if l and not l.startswith("#")]
    bad = [l for l in out if TARGET in l.casefold()]
    if bad:
        raise SystemExit(f"Négatifs contenant « {TARGET} » : {bad}")
    return out


def to_16k(x: np.ndarray, sr: int) -> np.ndarray:
    if x.ndim > 1:
        x = x.mean(axis=1)
    if sr != SR:
        g = gcd(sr, SR)
        x = resample_poly(x, SR // g, sr // g)
    return np.clip(x, -1.0, 1.0).astype(np.float32)


def trim(x: np.ndarray, margin: float = 0.1) -> np.ndarray:
    """Coupe le silence avant/après (fenêtres de 10 ms sous 5 % du pic)."""
    hop = SR // 100
    frames = [np.sqrt(np.mean(x[i : i + hop] ** 2)) for i in range(0, len(x) - hop + 1, hop)]
    if not frames or max(frames) == 0:
        return x
    loud = [i for i, r in enumerate(frames) if r >= 0.05 * max(frames)]
    pad = int(margin * SR)
    return x[max(0, loud[0] * hop - pad) : min(len(x), (loud[-1] + 1) * hop + pad)]


def write(path: Path, x: np.ndarray) -> None:
    sf.write(path, (x * 32767).astype(np.int16), SR, subtype="PCM_16")


def synth(job):
    """Rend True (créé), False (déjà là ou trop court) ou le texte de l'erreur."""
    try:
        return _synth(job)
    except Exception as e:
        return f"{type(e).__name__}: {e}"


def _synth(job) -> bool:
    path, text, vi, speaker, ls, ns, nw, max_s = job
    path = Path(path)
    if path.exists():
        return False
    for _ in range(TRIES):
        x = _render(text, vi, speaker, ls, ns, nw)
        if max_s is None or len(x) <= max_s * SR:
            break
    else:
        return False
    if len(x) < SR // 5:
        return False
    write(path, x)
    return True


def _render(text, vi, speaker, ls, ns, nw) -> np.ndarray:
    voice, n = _voices[vi]
    raw = b"".join(
        voice.synthesize_stream_raw(
            text,
            speaker_id=speaker if n > 1 else None,
            length_scale=ls,
            noise_scale=ns,
            noise_w=nw,
            sentence_silence=0.0,
        )
    )
    x = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    return trim(to_16k(x, voice.config.sample_rate))


def jobs_for(split_dir: Path, n: int, texts: list, rng: random.Random, voice_speakers: list, prefix: str,
             max_s: float | None = None) -> list:
    split_dir.mkdir(parents=True, exist_ok=True)
    out = []
    for i in range(n):
        vi = rng.randrange(len(voice_speakers))
        out.append(
            (
                str(split_dir / f"{prefix}_{i:06d}.wav"),
                rng.choice(texts),
                vi,
                rng.randrange(voice_speakers[vi]),
                rng.uniform(0.75, 1.35),  # débit
                rng.uniform(0.4, 0.75),  # variation de timbre (au-delà, Piper bredouille plus souvent)
                rng.uniform(0.5, 0.9),  # variation de rythme
                max_s,
            )
        )
    return out


def add_own(recordings: Path, out: Path, n_pos: int, rng: random.Random, globs: list) -> None:
    own = sorted(f for g in globs for f in recordings.glob(g)) if recordings else []
    if not own:
        print(f"Aucun enregistrement personnel ({', '.join(globs)}) : positifs 100 % synthétiques.")
        return
    rng.shuffle(own)
    cut = max(1, len(own) // 5)
    test, train = own[:cut], own[cut:]
    dup = max(1, round(0.1 * n_pos / max(1, len(train))))
    for split, files, copies in (("positive_train", train, dup), ("positive_test", test, 1)):
        for f in files:
            x, sr = sf.read(f, dtype="float32", always_2d=False)
            x = trim(to_16k(x, sr))
            for c in range(copies):
                p = out / split / f"own_{f.stem}_{c:03d}.wav"
                if not p.exists():
                    write(p, x)
    print(f"Enregistrements personnels : {len(train)} × {dup} en entraînement, {len(test)} en test.")


def add_hard_negatives(recordings: Path, out: Path, copies: int = 20) -> None:
    """Extraits captés par listen_live.py (faux et quasi-déclenchements) : négatifs les plus utiles."""
    found = [f for d in ("false_triggers", "near_misses") for f in sorted((recordings / d).glob("*.wav"))] if recordings else []
    for f in found:
        x, sr = sf.read(f, dtype="float32", always_2d=False)
        x = to_16k(x, sr)
        for c in range(copies):
            p = out / "negative_train" / f"live_{f.stem}_{c:02d}.wav"
            if not p.exists():
                write(p, x)
    if found:
        print(f"Extraits de l'écoute en direct : {len(found)} × {copies} en négatifs d'entraînement.")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True, help="dossier de travail ($WW_HOME)")
    ap.add_argument("--recordings", type=Path, default=None)
    ap.add_argument("--n-train", type=int, default=20000)
    ap.add_argument("--n-test", type=int, default=2000)
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--seed", type=int, default=8)
    ap.add_argument("--model", choices=sorted(MODELS), default="iris")
    a = ap.parse_args()

    out = a.ww / "output" / a.model
    positive_texts, own_globs, max_s = MODELS[a.model]
    negatives = read_negatives(Path(__file__).with_name("negatives_fr.txt"))
    voice_dir = a.ww / "voices"

    speakers = speakers_of(voice_dir)
    if not speakers:
        raise SystemExit(f"Aucune voix dans {voice_dir} : lance setup.sh.")
    print(f"{len(speakers)} voix, {sum(speakers)} locuteurs.")

    # Positifs synthétiques trop longs d'une génération précédente : supprimés, donc refaits ci-dessous.
    old = [f for d in ("positive_train", "positive_test") for f in (out / d).glob("pos_*.wav") if sf.info(f).duration > max_s]
    for f in old:
        f.unlink()
    if old:
        print(f"{len(old)} positifs de plus de {max_s} s supprimés, à refaire.")

    rng = random.Random(a.seed)
    extra = 1.3  # ≈ 1 positif sur 4 reste trop long après TRIES tirages (souvent le même locuteur) : on en prévoit plus
    jobs = (
        jobs_for(out / "positive_train", int(a.n_train * extra), positive_texts, rng, speakers, "pos", max_s)
        + jobs_for(out / "positive_test", int(a.n_test * extra), positive_texts, rng, speakers, "pos", max_s)
        + jobs_for(out / "negative_train", a.n_train, negatives, rng, speakers, "neg")
        + jobs_for(out / "negative_test", a.n_test, negatives, rng, speakers, "neg")
    )
    done, errors = 0, 0
    # Un worker Piper grossit sans cesse (≈ 1,4 Go après 30 min) : on le recycle tous les 400 clips,
    # sinon WSL manque de mémoire, tue un worker, et le Pool attend pour toujours ses clips perdus.
    ctx = mp.get_context("spawn")
    with ctx.Pool(a.workers, initializer=load_voices, initargs=(str(voice_dir),), maxtasksperchild=400) as pool:
        for i, made in enumerate(pool.imap_unordered(synth, jobs, chunksize=32), 1):
            if isinstance(made, str):
                errors += 1
                if errors <= 3:
                    print(f"  erreur : {made}", flush=True)
                if errors == 50 and done == 0:
                    raise SystemExit("50 erreurs sans aucun clip : arrêt.")
            else:
                done += made
            if i % 500 == 0:
                print(f"  {i}/{len(jobs)} ({done} créés)", flush=True)
    if errors:
        print(f"{errors} clips en erreur.")
    print(f"{done} clips générés ({len(jobs) - done} déjà présents ou trop courts).")

    add_own(a.recordings, out, a.n_train, rng, own_globs)
    add_hard_negatives(a.recordings, out)
    for d in ("positive_train", "positive_test", "negative_train", "negative_test"):
        print(f"{d} : {len(list((out / d).glob('*.wav')))}")
    shutil.copy(Path(__file__).with_name("negatives_fr.txt"), out / "negatives_fr.txt")


if __name__ == "__main__":
    main()
