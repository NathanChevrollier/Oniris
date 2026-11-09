"""Convertit les sources audio brutes en WAV mono 16 kHz 16 bits, le format qu'attend openWakeWord.

- data/bal_train09.tar (AudioSet, .flac)        -> data/background/
- data/extra_src/ (tes musiques, .mp3/.flac/.wav) -> data/background/
- data/mit_rirs_raw/ (wav ou parquet)           -> data/rir/
- <recordings>/ambient*.wav (bruit de ta pièce)  -> data/ambient/ (morceaux de 10 s)

Relançable : un fichier déjà converti est sauté.
"""
import argparse
import io
import tarfile
from math import gcd
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

SR = 16000
AUDIO = {".wav", ".flac", ".mp3", ".ogg"}


def to_16k(x: np.ndarray, sr: int) -> np.ndarray:
    if x.ndim > 1:
        x = x.mean(axis=1)
    if sr != SR:
        g = gcd(sr, SR)
        x = resample_poly(x, SR // g, sr // g)
    return np.clip(x, -1.0, 1.0)


def write(path: Path, x: np.ndarray) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    sf.write(path, (x * 32767).astype(np.int16), SR, subtype="PCM_16")


def convert_file(src: Path, dst: Path) -> bool:
    if dst.exists():
        return False
    try:
        x, sr = sf.read(src, dtype="float32", always_2d=False)
    except Exception as e:  # fichier illisible : on le saute, on ne bloque pas tout
        print(f"  ignoré {src.name} : {e}")
        return False
    if len(x) < sr // 4:
        return False
    write(dst, to_16k(x, sr))
    return True


def convert_dir(src: Path, dst: Path) -> int:
    n = 0
    for f in sorted(src.rglob("*")):
        if f.suffix.lower() in AUDIO:
            n += convert_file(f, dst / (f.stem + ".wav"))
    return n


def rirs_from_parquet(src: Path, dst: Path) -> int:
    import pyarrow.parquet as pq

    n = 0
    for p in sorted(src.rglob("*.parquet")):
        table = pq.read_table(p)
        col = next((c for c in table.column_names if c.lower() == "audio"), None)
        if col is None:
            print(f"  {p.name} : pas de colonne audio ({table.column_names})")
            continue
        for i, cell in enumerate(table.column(col).to_pylist()):
            out = dst / f"{p.stem}_{i:04d}.wav"
            if out.exists() or not cell or not cell.get("bytes"):
                continue
            x, sr = sf.read(io.BytesIO(cell["bytes"]), dtype="float32", always_2d=False)
            write(out, to_16k(x, sr))
            n += 1
    return n


def split_ambient(recordings: Path, dst: Path, secs: int = 10) -> int:
    n = 0
    for f in sorted(recordings.glob("ambient*.wav")):
        x, sr = sf.read(f, dtype="float32", always_2d=False)
        x = to_16k(x, sr)
        step = secs * SR
        for i in range(0, len(x) - step + 1, step):
            out = dst / f"{f.stem}_{i // step:04d}.wav"
            if not out.exists():
                write(out, x[i : i + step])
                n += 1
    return n


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True, help="dossier de travail ($WW_HOME)")
    ap.add_argument("--recordings", type=Path, default=None, help="dossier de tes enregistrements (record_samples.py)")
    a = ap.parse_args()
    data = a.ww / "data"

    tar = data / "bal_train09.tar"
    raw = data / "audioset_raw"
    if tar.exists() and not raw.exists():
        print("Extraction AudioSet…")
        with tarfile.open(tar) as t:
            t.extractall(raw, filter="data")
    if raw.exists():
        print(f"AudioSet : {convert_dir(raw, data / 'background')} fichiers")
    if (data / "extra_src").exists():
        print(f"Tes sons : {convert_dir(data / 'extra_src', data / 'background')} fichiers")

    rraw = data / "mit_rirs_raw"
    if rraw.exists():
        n = convert_dir(rraw, data / "rir") + rirs_from_parquet(rraw, data / "rir")
        print(f"Réverbérations : {n} fichiers")

    (data / "ambient").mkdir(parents=True, exist_ok=True)
    if a.recordings and a.recordings.exists():
        print(f"Ambiance de ta pièce : {split_ambient(a.recordings, data / 'ambient')} morceaux")

    for d in ("background", "rir"):
        count = len(list((data / d).glob("*.wav"))) if (data / d).exists() else 0
        print(f"data/{d} : {count} WAV")
        if count == 0:
            raise SystemExit(f"data/{d} est vide : l'augmentation échouerait.")


if __name__ == "__main__":
    main()
