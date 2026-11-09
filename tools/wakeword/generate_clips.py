"""Génère les clips d'entraînement en français avec les voix Piper, à la place de l'étape --generate_clips
d'openWakeWord (qui ne sait produire que de l'anglais).

Sortie, dans le dossier attendu par openwakeword/train.py :
  output/iris/positive_train  positive_test  negative_train  negative_test   (WAV mono 16 kHz 16 bits)

Tes propres enregistrements (record_samples.py) sont ajoutés aux positifs : 80 % en entraînement
(dupliqués pour peser ~10 % des positifs, l'augmentation les rend tous différents), 20 % en test.
Relançable : un clip déjà présent est sauté.
"""
import argparse
import random
import shutil
from math import gcd
from multiprocessing import Pool
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

SR = 16000
TARGET = "iris"
POSITIVE_TEXTS = ["Iris", "Iris.", "Iris !", "Iris ?", "Iris,", "Iris…", "iris"]

_voices = []  # (PiperVoice, nb de locuteurs), chargées une fois par processus


def load_voices(voice_dir: str) -> None:
    from piper import PiperVoice

    _voices.clear()  # un fork hérite de la liste du parent
    for p in sorted(Path(voice_dir).glob("*.onnx")):
        v = PiperVoice.load(str(p))
        _voices.append((v, max(1, int(v.config.num_speakers or 1))))


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


def synth(job) -> bool:
    path, text, vi, speaker, ls, ns, nw = job
    path = Path(path)
    if path.exists():
        return False
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
    x = trim(to_16k(x, voice.config.sample_rate))
    if len(x) < SR // 5:
        return False
    write(path, x)
    return True


def jobs_for(split_dir: Path, n: int, texts: list, rng: random.Random, voice_speakers: list, prefix: str) -> list:
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
                rng.uniform(0.4, 0.9),  # variation de timbre
                rng.uniform(0.5, 1.0),  # variation de rythme
            )
        )
    return out


def add_own(recordings: Path, out: Path, n_pos: int, rng: random.Random) -> None:
    own = sorted(recordings.glob("iris_*.wav")) if recordings else []
    if not own:
        print("Aucun enregistrement personnel (iris_*.wav) : positifs 100 % synthétiques.")
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


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True, help="dossier de travail ($WW_HOME)")
    ap.add_argument("--recordings", type=Path, default=None)
    ap.add_argument("--n-train", type=int, default=20000)
    ap.add_argument("--n-test", type=int, default=2000)
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--seed", type=int, default=8)
    a = ap.parse_args()

    out = a.ww / "output" / "iris"
    negatives = read_negatives(Path(__file__).with_name("negatives_fr.txt"))
    voice_dir = a.ww / "voices"

    load_voices(str(voice_dir))  # pour connaître les locuteurs ; les workers rechargent
    if not _voices:
        raise SystemExit(f"Aucune voix dans {voice_dir} : lance setup.sh.")
    speakers = [n for _, n in _voices]
    print(f"{len(_voices)} voix, {sum(speakers)} locuteurs.")

    rng = random.Random(a.seed)
    jobs = (
        jobs_for(out / "positive_train", a.n_train, POSITIVE_TEXTS, rng, speakers, "pos")
        + jobs_for(out / "positive_test", a.n_test, POSITIVE_TEXTS, rng, speakers, "pos")
        + jobs_for(out / "negative_train", a.n_train, negatives, rng, speakers, "neg")
        + jobs_for(out / "negative_test", a.n_test, negatives, rng, speakers, "neg")
    )
    with Pool(a.workers, initializer=load_voices, initargs=(str(voice_dir),)) as pool:
        done = 0
        for i, made in enumerate(pool.imap_unordered(synth, jobs, chunksize=32), 1):
            done += made
            if i % 2000 == 0:
                print(f"  {i}/{len(jobs)}")
    print(f"{done} clips générés ({len(jobs) - done} déjà présents ou trop courts).")

    add_own(a.recordings, out, a.n_train, rng)
    for d in ("positive_train", "positive_test", "negative_train", "negative_test"):
        print(f"{d} : {len(list((out / d).glob('*.wav')))}")
    shutil.copy(Path(__file__).with_name("negatives_fr.txt"), out / "negatives_fr.txt")


if __name__ == "__main__":
    main()
