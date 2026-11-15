"""Tests des parties pures du réentraînement (nettoyage, découpage stable, règle de promotion), sans données lourdes :
WAV synthétiques dans un dossier temporaire. Ni openWakeWord ni scipy requis.

  py -3.12 tools\\wakeword\\test_retrain.py        (ou python -m pytest tools/wakeword/test_retrain.py)
"""
import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path

import numpy as np
import soundfile as sf

sys.path.insert(0, str(Path(__file__).resolve().parent))
import clean_data as cd  # noqa: E402
import gate  # noqa: E402
from generate_clips import add_own  # noqa: E402

SR = 16000
rng = np.random.default_rng(0)


def noise(s, level=0.002):
    return (rng.normal(0, level, int(s * SR))).astype(np.float32)


def word(s=1.8, at=0.5, dur=0.6, amp=0.3, seed=0):
    """Bruit faible avec une « parole » (sinus modulés) au milieu."""
    x = noise(s)
    t = np.arange(int(dur * SR)) / SR
    x[int(at * SR) : int(at * SR) + len(t)] += amp * np.sin(2 * np.pi * (180 + 40 * seed) * t) * np.sin(np.pi * t / dur)
    return x


def write(p: Path, x, sr=SR):
    p.parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(p), x, sr, subtype="PCM_16")


def tree_hash(d: Path) -> str:
    h = hashlib.sha1()
    for f in sorted(d.rglob("*")):
        if f.is_file():
            h.update(f.relative_to(d).as_posix().encode() + f.read_bytes() + str(f.stat().st_mtime_ns).encode())
    return h.hexdigest()


def make_recordings(rec: Path):
    for i in range(10):
        write(rec / f"dis_{i:04d}.wav", word(seed=i))
    write(rec / "he_0100.wav", noise(1.8, 0.0001))  # silencieux
    clipped = word(amp=0.3)
    clipped[10000:11000] = 1.0
    write(rec / "he_0101.wav", clipped)  # saturé
    write(rec / "he_0102.wav", noise(1.8, 0.01))  # bruit sans pic de parole
    write(rec / "he_0103.wav", word()[: int(0.2 * SR)])  # trop court
    (rec / "he_0104.wav").write_bytes(b"pas un wav")  # illisible
    write(rec / "learn" / "pos" / "a.wav", word(1.6, at=0.6, seed=3))
    write(rec / "learn" / "pos" / "copie.wav", sf.read(str(rec / "dis_0001.wav"), dtype="float32")[0])  # doublon d'un guidé
    write(rec / "learn" / "neg" / "b.wav", word(3.0, at=1.0, seed=7))
    write(rec / "false_triggers" / "fa_1.wav", word(3.0, at=2.0, seed=8))
    write(rec / "ambient_1.wav", noise(12, 0.01))
    os.utime(rec / "dis_0009.wav", (1e9, 1e9))  # prise bien plus ancienne : une autre session


def test_decide_rules():
    with tempfile.TemporaryDirectory() as t:
        p = Path(t) / "x.wav"
        write(p, word())
        a = cd.analyse(p)
        assert cd.decide(a, "guide", 1.6) == ("garde", ""), cd.decide(a, "guide", 1.6)
        write(p, noise(1.8, 0.0001))
        assert cd.decide(cd.analyse(p), "guide", 1.6) == ("rejet", "silencieux")
        write(p, noise(1.8, 0.01))
        assert cd.decide(cd.analyse(p), "guide", 1.6) == ("quarantaine", "pas_de_parole")
        assert cd.decide(cd.analyse(p), "faux_declenchement", 1.6)[0] == "garde", "un négatif n'a pas besoin de parole"
        write(p, word(4.0, at=0.3, dur=3.0))
        assert cd.decide(cd.analyse(p), "guide", 1.6) == ("rejet", "trop_long")
        write(p, word(3.0, at=0.2, dur=2.5))
        assert cd.decide(cd.analyse(p), "guide", 1.6) == ("quarantaine", "parole_trop_longue")
        write(p, noise(12, 0.01))
        assert cd.decide(cd.analyse(p), "ambiance", 1.6) == ("garde", "")
    assert cd.decide({"erreur": "x"}, "guide", 1.6) == ("rejet", "illisible")


def test_fingerprint_ignores_name_and_container():
    with tempfile.TemporaryDirectory() as t:
        x = np.round(word() * 32767).astype(np.int16)  # mêmes échantillons 16 bits dans les deux conteneurs
        sf.write(str(Path(t) / "a.wav"), x, SR, subtype="PCM_16")
        sf.write(str(Path(t) / "b.flac"), x, SR, subtype="PCM_16")
        assert cd.analyse(Path(t) / "a.wav")["empreinte"] == cd.analyse(Path(t) / "b.flac")["empreinte"]


def test_clean_end_to_end_and_never_touches_recordings():
    with tempfile.TemporaryDirectory() as t:
        rec, ww = Path(t) / "rec", Path(t) / "ww"
        make_recordings(rec)
        before = tree_hash(rec)
        out, split = ww / "clean" / "dis_iris", ww / "split_dis_iris.json"
        r = cd.clean(rec, ww, out, "dis_iris", split)
        assert tree_hash(rec) == before, "les enregistrements ne doivent jamais changer"
        by = {f["source"]: f for f in r["fichiers"]}
        assert by["he_0100.wav"]["raison"] == "silencieux"
        assert by["he_0101.wav"]["raison"] == "sature"
        assert by["he_0102.wav"]["decision"] == "quarantaine"
        assert by["he_0103.wav"]["raison"] == "trop_court"
        assert by["he_0104.wav"]["raison"] == "illisible"
        assert by["learn/pos/copie.wav"]["raison"].startswith("doublon de dis_0001.wav")
        assert (out / "quarantaine" / "pas_de_parole" / "he_0102.wav").exists()
        rs = out / "recordings"
        assert (rs / "learn" / "pos" / "a.wav").exists() and not (rs / "test" / "a.wav").exists(), "appris : entraînement seulement"
        assert (rs / "learn" / "neg" / "b.wav").exists() and (rs / "false_triggers" / "fa_1.wav").exists()
        assert (rs / "ambient_1.wav").exists()
        test = {f.name for f in (rs / "test").glob("*.wav")}
        train = {f.name for f in rs.glob("dis_*.wav")}
        assert test and train and not test & train and len(test | train) == 10
        # Deux prises : la session ancienne (dis_0009) n'est jamais coupée en deux, et la seconde non plus.
        prises = {}
        for f in r["fichiers"]:
            if f["type"] == "guide" and f["decision"] == "garde":
                prises.setdefault(f["prise"], set()).add(f["split"])
        assert len(prises) == 2
        # Relance avec un nouveau fichier : le découpage déjà fait ne bouge pas, les copies sont refaites à l'identique.
        h1 = (out / "data_hash.txt").read_text()
        r2 = cd.clean(rec, ww, out, "dis_iris", split)
        assert (out / "data_hash.txt").read_text() == h1 and r2["nouveaux_dans_decoupage"] == 0
        write(rec / "dis_0050.wav", word(seed=11))
        cd.clean(rec, ww, out, "dis_iris", split)
        assert {f.name for f in (rs / "test").glob("*.wav")} - {"dis_0050.wav"} == test
        assert (out / "data_hash.txt").read_text() != h1
        # Décisions humaines.
        (out / "garder.txt").write_text("he_0102.wav\n", encoding="utf-8")
        (out / "exclure.txt").write_text("false_triggers/fa_1.wav\n", encoding="utf-8")
        r3 = {f["source"]: f for f in cd.clean(rec, ww, out, "dis_iris", split)["fichiers"]}
        assert r3["he_0102.wav"]["decision"] == "garde" and r3["false_triggers/fa_1.wav"]["raison"] == "exclu_par_toi"
        assert not (rs / "false_triggers" / "fa_1.wav").exists()
        # generate_clips lit ce découpage tel quel.
        o = Path(t) / "gen"
        for d in ("positive_train", "positive_test"):
            (o / d).mkdir(parents=True)
        add_own(rs, o, 100, ["dis_*.wav", "he_*.wav"])
        got_test = {f.name.rsplit("_", 1)[0][4:] + ".wav" for f in (o / "positive_test").glob("own_*.wav")}
        assert got_test == {f.name for f in (rs / "test").glob("*.wav")}
        assert not list((o / "positive_test").glob("own_learn_*"))


def test_legacy_split_is_reused():
    with tempfile.TemporaryDirectory() as t:
        rec, ww = Path(t) / "rec", Path(t) / "ww"
        for i in range(6):
            write(rec / f"dis_{i:04d}.wav", word(seed=i))
        old = ww / "output" / "dis_iris"
        write(old / "positive_test" / "own_dis_0004_000.wav", word())
        write(old / "positive_train" / "own_dis_0001_003.wav", word())
        write(old / "positive_train" / "own_learn_zz_000.wav", word())
        r = cd.clean(rec, ww, ww / "clean", "dis_iris", ww / "split.json")
        sp = {f["source"]: f["split"] for f in r["fichiers"]}
        assert sp["dis_0004.wav"] == "test" and sp["dis_0001.wav"] == "train"
        assert json.loads((ww / "split.json").read_text(encoding="utf-8"))


def test_out_inside_recordings_is_refused():
    with tempfile.TemporaryDirectory() as t:
        rec = Path(t) / "rec"
        rec.mkdir()
        for out in (rec, rec / "clean"):
            try:
                cd.clean(rec, Path(t), out, "dis_iris", Path(t) / "s.json")
            except SystemExit:
                continue
            raise AssertionError(f"--out {out} accepté")


def test_probe_rules():
    assert cd.decide_probe("appris_pos", 0.001, None, None) == "score_tres_bas"
    assert cd.decide_probe("guide", 0.0, None, None) == "", "les guidés sont la référence"
    assert cd.decide_probe("faux_declenchement", 0.999, 0.97, 0.93) == "contient_iris"
    assert cd.decide_probe("faux_declenchement", 0.999, 0.80, 0.93) == "", "score haut seul : vrai faux déclenchement"
    assert cd.decide_probe("appris_neg", 0.1, 0.99, 0.93) == ""
    assert cd.decide_probe("appris_neg", 0.999, None, None) == "", "sans référence, pas de quarantaine"


def curve(points):
    """{seuil: (faux/h, rappel ta voix)} -> courbe au format de gate.measure."""
    return {str(t): {"faux_par_heure": f, "rappel_ta_voix": r, "rappel_synth": r} for t, (f, r) in points.items()}


def test_promotion_rule():
    old = curve({0.5: (3.0, 0.9), 0.9: (0.4, 0.6), 0.998: (0.08, 0.5)})
    better = curve({0.5: (1.0, 0.9), 0.9: (0.3, 0.8), 0.998: (0.05, 0.7)})
    worse_recall = curve({0.5: (1.0, 0.9), 0.9: (0.3, 0.5), 0.998: (0.05, 0.4)})
    no_target = curve({0.5: (3.0, 0.95), 0.9: (0.6, 0.9), 0.998: (0.2, 0.8)})
    assert gate.point(old, 0.5) == dict(old["0.9"], seuil=0.9, tenu=True)
    assert gate.point(no_target, 0.1)["seuil"] == 0.998 and not gate.point(no_target, 0.1)["tenu"]
    assert gate.admissible(better, old) == []
    assert gate.admissible(old, old) == [], "à égalité, au moins aussi bon"
    assert any("rappel" in w for w in gate.admissible(worse_recall, old))
    assert any("ne tient pas 0.1" in w for w in gate.admissible(no_target, old))
    win, refus = gate.choose(old, {"a": worse_recall, "b": better, "c": no_target})
    assert win == "b" and refus["a"] and refus["c"]
    assert gate.choose(old, {"a": worse_recall})[0] is None, "aucun meilleur : l'ancien reste"
    assert gate.choose(None, {"a": worse_recall})[0] == "a", "pas de modèle livré : le meilleur candidat"
    # Ni l'un ni l'autre ne tient 0,1 : le candidat ne doit pas faire plus de faux déclenchements.
    o2 = curve({0.5: (0.4, 0.8), 0.998: (0.2, 0.6)})
    c2 = curve({0.5: (0.4, 0.9), 0.998: (0.3, 0.9)})
    assert any("faux/h" in w for w in gate.admissible(c2, o2))


def test_advised_threshold_and_rust_line():
    c = curve({0.5: (1.0, 1.0), 0.99: (0.1, 0.92), 0.998: (0.05, 0.8)})
    assert gate.advised_threshold(c)[0] == 0.99
    assert gate.advised_threshold(curve({0.5: (0.05, 0.5), 0.9: (0.0, 0.4)}))[0] == 0.5
    with tempfile.TemporaryDirectory() as t:
        f = Path(t) / "src-tauri" / "src" / "voice" / "wake.rs"
        f.parent.mkdir(parents=True)
        f.write_text("//! x\n\npub const WAKE_THRESHOLD: f32 = 0.998;\n", encoding="utf-8")
        assert gate.wake_threshold_line(Path(t)) == ("src-tauri/src/voice/wake.rs", 3, 0.998)
    repo = Path(__file__).resolve().parents[2]
    loc = gate.wake_threshold_line(repo)
    assert loc and loc[0].endswith("wake.rs"), loc


def test_backup_and_restore():
    with tempfile.TemporaryDirectory() as t:
        repo, bk = Path(t) / "repo", Path(t) / "bk"
        p = gate.paths(repo, "dis_iris")
        p["model"].parent.mkdir(parents=True)
        p["model"].write_bytes(b"ancien")
        p["sums"].write_text("x  dis_iris.onnx\n")
        (p["fixture"]).mkdir(parents=True)
        (p["fixture"] / "scores.json").write_text("{}")
        saved = gate.backup(repo, "dis_iris", bk)
        p["model"].write_bytes(b"nouveau")
        (p["fixture"] / "scores.json").write_text("cassé")
        gate.restore(repo, "dis_iris", saved.parent)
        assert p["model"].read_bytes() == b"ancien" and (p["fixture"] / "scores.json").read_text() == "{}"
        assert gate.backup(Path(t) / "vide", "dis_iris", bk / "2") is None


if __name__ == "__main__":
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in tests:
        fn()
        print(f"ok  {fn.__name__}")
    print(f"{len(tests)} tests réussis")
