"""Convertit les sources audio brutes en WAV mono 16 kHz 16 bits, le format qu'attend openWakeWord.

- data/audioset/*.parquet (AudioSet)            -> data/background/
- data/extra_src/ (tes musiques, .mp3/.flac/.wav) -> data/background/
- data/mit_rirs_raw/ (wav ou parquet)           -> data/rir/
- <recordings>/ambient*.wav (bruit de ta pièce)  -> data/ambient/ (morceaux de 10 s)

Relançable : un fichier déjà converti est sauté.
"""
import argparse
import io
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


def from_parquet(src: Path, dst: Path) -> int:
    """Colonne « audio » au format Hugging Face ({bytes, path}), lue par lots pour ménager la mémoire."""
    import pyarrow.parquet as pq

    n = 0
    for p in sorted(src.rglob("*.parquet")):
        f = pq.ParquetFile(p)
        col = next((c for c in f.schema_arrow.names if c.lower() == "audio"), None)
        if col is None:
            print(f"  {p.name} : pas de colonne audio ({f.schema_arrow.names})")
            continue
        i = 0
        for batch in f.iter_batches(batch_size=64, columns=[col]):
            for cell in batch.column(0).to_pylist():
                out = dst / f"{p.stem}_{i:05d}.wav"
                i += 1
                if out.exists() or not cell or not cell.get("bytes"):
                    continue
                try:
                    x, sr = sf.read(io.BytesIO(cell["bytes"]), dtype="float32", always_2d=False)
                except Exception as e:  # extrait illisible : on le saute
                    print(f"  ignoré {out.name} : {e}")
                    continue
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

    if (data / "audioset").exists():
        print("AudioSet (quelques minutes)…")
        print(f"AudioSet : {from_parquet(data / 'audioset', data / 'background')} fichiers")
    if (data / "extra_src").exists():
        print(f"Tes sons : {convert_dir(data / 'extra_src', data / 'background')} fichiers")

    rraw = data / "mit_rirs_raw"
    if rraw.exists():
        n = convert_dir(rraw, data / "rir") + from_parquet(rraw, data / "rir")
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
