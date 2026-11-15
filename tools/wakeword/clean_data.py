"""Collecte et nettoie toutes tes données avant un réentraînement (retrain_all.sh). Tes enregistrements ne sont
jamais modifiés ni supprimés : les fichiers retenus sont COPIÉS (WAV 16 kHz mono 16 bits) dans un dossier de travail,
au format qu'attend generate_clips.py ; les suspects sont copiés en quarantaine ; les autres sont seulement ignorés.

  python clean_data.py --ww ~/oniris-wakeword --recordings /mnt/c/Users/<toi>/oniris-wakeword/recordings \
      [--model dis_iris] [--score-model <dépôt>/src-tauri/resources/wake/dis_iris.onnx]

Sources (dans --recordings) :
  dis_*.wav, he_*.wav (iris_*.wav)  enregistrements guidés (record_samples.py) : entraînement ET test
  learn/pos/*.wav                   positifs du mode apprentissage d'Oniris : entraînement seulement
  learn/neg/*.wav, false_triggers/, near_misses/   négatifs durs (Oniris, listen_live.py)
  ambient*.wav                      ambiance de ta pièce (fond sonore)

Sortie (--out, défaut $WW/clean/<model>, refaite à chaque passage) :
  recordings/            ce que lit generate_clips.py : guidés d'entraînement à la racine, guidés de test dans test/
  quarantaine/<raison>/  copies des fichiers suspects, à écouter
  nettoyage.json / .md   chaque fichier, sa décision et sa raison
  data_hash.txt          empreinte de l'ensemble retenu (si elle change, les caractéristiques sont à refaire)
  garder.txt / exclure.txt   TES décisions (un nom de fichier ou chemin relatif par ligne) : garder.txt sort un
                         fichier de quarantaine, exclure.txt écarte un fichier.
Découpage train/test figé : $WW/split_<model>.json (empreinte audio -> train/test), jamais changé pour un fichier déjà vu.
"""
import argparse
import hashlib
import json
import shutil
import time
from pathlib import Path

import numpy as np
import soundfile as sf

from generate_clips import MODELS, SR, to_16k, trim

# Bornes de durée (s) par source ; ambiance : pas de maximum.
DURATION = {"guide": (0.5, 3.0), "appris_pos": (0.9, 3.0), "appris_neg": (0.9, 3.5),
            "faux_declenchement": (0.9, 4.0), "quasi_declenchement": (0.9, 4.0), "ambiance": (5.0, None)}
ROLE = {"guide": "pos", "appris_pos": "pos", "appris_neg": "neg", "faux_declenchement": "neg",
        "quasi_declenchement": "neg", "ambiance": "amb"}
# Dossier de sortie (dans recordings/) qui garde la disposition attendue par generate_clips.py.
DEST = {"appris_pos": "learn/pos", "appris_neg": "learn/neg", "faux_declenchement": "false_triggers",
        "quasi_declenchement": "near_misses", "guide": "", "ambiance": ""}
MIN_PEAK = 10 ** (-45 / 20)  # pic sous -45 dBFS : rien d'audible
MIN_RMS_DB = -60.0
AMB_MIN_RMS_DB = -75.0  # ambiance : seul un micro mort descend si bas
CLIP_LEVEL, CLIP_FRAC = 0.999, 0.001  # plus de 0,1 % d'échantillons à pleine échelle : saturé
SPEECH_DYN_DB, SPEECH_MIN_S = 10.0, 0.15  # pic d'énergie de parole au-dessus du fond, durée active minimale
TEST_SHARE = 20  # % des prises guidées (nouvelles) en test
SESSION_GAP_S = 20 * 60  # deux fichiers guidés à plus de 20 min d'écart : deux prises différentes
POS_MIN_SCORE = 0.02  # positif appris que le modèle actuel ne voit pas du tout : à vérifier
NEG_MIN_SCORE = 0.5  # négatif qui déclenche ET ressemble à tes « Dis Iris » : à vérifier
MIN_REFS = 8  # enregistrements guidés nécessaires pour juger la ressemblance


def collect(rec: Path, model: str) -> list:
    """Tous les fichiers candidats : [{source, kind, path}], dans un ordre stable."""
    globs = MODELS[model][1]
    found = [("guide", f) for g in globs for f in rec.glob(g)]
    found += [("appris_pos", f) for f in (rec / "learn" / "pos").glob("*.wav")]
    found += [("appris_neg", f) for f in (rec / "learn" / "neg").glob("*.wav")]
    found += [("faux_declenchement", f) for f in (rec / "false_triggers").glob("*.wav")]
    found += [("quasi_declenchement", f) for f in (rec / "near_misses").glob("*.wav")]
    found += [("ambiance", f) for f in rec.glob("ambient*.wav")]
    items = [{"source": f.relative_to(rec).as_posix(), "kind": k, "path": f} for k, f in found if f.is_file()]
    return sorted(items, key=lambda i: (list(DURATION).index(i["kind"]), i["source"]))


def activity(x: np.ndarray) -> tuple:
    """(écart pic/fond en dB, durée active en s, étendue active en s) sur des trames de 20 ms."""
    hop = SR // 50
    n = len(x) // hop
    if n < 2:
        return 0.0, 0.0, 0.0
    db = 10 * np.log10(np.mean(x[: n * hop].reshape(n, hop).astype(np.float64) ** 2, axis=1) + 1e-12)
    floor, top = float(np.percentile(db, 10)), float(db.max())
    idx = np.flatnonzero(db >= max(floor + SPEECH_DYN_DB, top - 35))
    span = (idx[-1] - idx[0] + 1) / 50 if len(idx) else 0.0
    return top - floor, len(idx) / 50, span


def analyse(path: Path) -> dict:
    """Mesures d'un fichier ; « x » est le signal 16 kHz mono (float32), absent si illisible."""
    try:
        info = sf.info(str(path))
        raw, sr = sf.read(str(path), dtype="float32", always_2d=True)
    except Exception as e:  # fichier illisible : décision « rejet », jamais d'arrêt
        return {"erreur": f"{type(e).__name__}: {e}"}
    m = {"sr": sr, "canaux": raw.shape[1], "format": info.subtype, "duree": round(len(raw) / sr, 3) if sr else 0.0}
    if len(raw) == 0:
        return m
    m["sature"] = float(np.mean(np.abs(raw).max(axis=1) >= CLIP_LEVEL))
    try:
        x = to_16k(raw[:, 0] if raw.shape[1] == 1 else raw, sr)
    except ImportError as e:  # scipy absent (Windows) : impossible de rééchantillonner
        return dict(m, erreur=f"rééchantillonnage impossible : {e}")
    m["x"] = x
    m["pic"] = float(np.abs(x).max())
    m["rms_db"] = float(10 * np.log10(np.mean(x.astype(np.float64) ** 2) + 1e-12))
    m["dyn_db"], m["parole_s"], m["etendue_s"] = activity(x)
    # Empreinte de l'audio décodé (pas du nom ni du conteneur) : clé des doublons et du découpage.
    m["empreinte"] = hashlib.sha1(np.round(x * 32767).astype(np.int16).tobytes()).hexdigest()[:16]
    m["natif"] = sr == SR and raw.shape[1] == 1 and info.subtype == "PCM_16" and info.format == "WAV"
    return m


def decide(m: dict, kind: str, max_speech_s: float) -> tuple:
    """(décision, raison) d'après les seules mesures : garde, rejet (ignoré) ou quarantaine (copié pour écoute)."""
    if "erreur" in m:
        return "rejet", "illisible"
    lo, hi = DURATION[kind]
    if m["duree"] < lo:
        return "rejet", "trop_court"
    if hi is not None and m["duree"] > hi:
        return "rejet", "trop_long"
    if kind == "ambiance":
        return ("rejet", "silencieux") if m["rms_db"] < AMB_MIN_RMS_DB else ("garde", "")
    if m["pic"] < MIN_PEAK or m["rms_db"] < MIN_RMS_DB:
        return "rejet", "silencieux"
    if m["sature"] > CLIP_FRAC:
        return "rejet", "sature"
    if ROLE[kind] == "pos":
        if m["dyn_db"] < SPEECH_DYN_DB or m["parole_s"] < SPEECH_MIN_S:
            return "quarantaine", "pas_de_parole"
        if m["etendue_s"] > max_speech_s + 0.3:
            return "quarantaine", "parole_trop_longue"
    return "garde", ""


def decide_probe(kind: str, score: float | None, sim: float | None, sim_thr: float | None) -> str:
    """Raison de quarantaine d'après le modèle actuel, ou "". Les guidés ne sont jamais jugés par le modèle : ce sont
    la référence (le modèle provisoire rate la moitié de ta voix, il écarterait tes positifs les plus utiles)."""
    if kind == "appris_pos" and score is not None and score < POS_MIN_SCORE:
        return "score_tres_bas"
    # Un faux déclenchement a par définition un score haut : seul un score haut ET une forte ressemblance à tes
    # « Dis Iris » enregistrés désignent un négatif qui contient sans doute vraiment le mot.
    if ROLE[kind] == "neg" and None not in (score, sim, sim_thr) and score >= NEG_MIN_SCORE and sim >= sim_thr:
        return "contient_iris"
    return ""


def sessions(items: list) -> None:
    """Numéro de prise des guidés : fichiers proches dans le temps (date de modification) = même prise."""
    guided = sorted((i for i in items if i["kind"] == "guide"), key=lambda i: i["mtime"])
    start, last = None, None
    for i in guided:
        if last is None or i["mtime"] - last > SESSION_GAP_S:
            start = time.strftime("%Y%m%d_%H%M", time.localtime(i["mtime"]))
        i["prise"], last = start, i["mtime"]


def bucket(key: str) -> int:
    return int(hashlib.sha1(key.encode()).hexdigest(), 16) % 100


def assign_split(guided: list, frozen: dict, legacy_test: set, legacy_train: set, model: str) -> list:
    """Découpage stable des guidés retenus ; rend les nouvelles entrées [(empreinte, entrée)].

    1. un fichier déjà vu (même empreinte audio) garde son côté, pour toujours ;
    2. au premier passage, le découpage du dernier entraînement (fichiers own_* de output/<model>) est repris,
       pour que l'ancien modèle et les nouveaux soient mesurés sur le même test ;
    3. une nouvelle prise part entière d'un côté (tirage par empreinte de la prise) ; s'il n'y a encore aucun test,
       tirage fichier par fichier (une seule prise ne peut pas être à la fois apprise et testée en entier)."""
    new = []
    for i in guided:
        stem = i["path"].stem
        if i["empreinte"] in frozen:
            continue
        side = "test" if stem in legacy_test else "train" if stem in legacy_train else None
        if side is None:
            side = "test" if bucket(f"{model}:{i['prise']}") < TEST_SHARE else "train"
        new.append((i["empreinte"], {"split": side, "fichier": i["source"], "prise": i["prise"],
                                     "depuis": time.strftime("%Y-%m-%d")}))
    sides = {v["split"] for v in frozen.values()} | {e["split"] for _, e in new}
    if new and "test" not in sides and not legacy_test:
        for fp, e in new:
            e["split"] = "test" if bucket(f"{model}:{fp}") < TEST_SHARE else "train"
    return new


def legacy_split(ww: Path, model: str) -> tuple:
    """Noms des guidés du dernier entraînement, d'après ses copies own_<nom>_NNN.wav (vide si aucun)."""
    out = ww / "output" / model
    names = lambda d: {f.stem.rsplit("_", 1)[0][4:] for f in (out / d).glob("own_*.wav") if not f.stem.startswith("own_learn_")}
    return names("positive_test"), names("positive_train")


class Probe:
    """Le modèle actuel sur chaque extrait : score maximal et ressemblance (cosinus des caractéristiques
    openWakeWord) avec tes enregistrements guidés. Ne tourne que dans le venv WSL."""

    def __init__(self, model_path: Path):
        import onnxruntime as ort
        from openwakeword.utils import AudioFeatures

        res = model_path.parent
        self.f = AudioFeatures(melspec_model_path=str(res / "melspectrogram.onnx"),
                               embedding_model_path=str(res / "embedding_model.onnx"), inference_framework="onnx")
        self.s = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
        self.inp = self.s.get_inputs()[0].name

    def windows(self, clips: list) -> list:
        """Fenêtres de 16 pas (1,28 s) de chaque extrait, posé 0,2 s avant la fin d'un fond faible commun."""
        n = int(max(2.5, max(len(c) for c in clips) / SR + 0.7) * SR)  # 2,5 s au moins : ≥ 16 pas de caractéristiques
        x = np.random.default_rng(8).normal(0, 60, (len(clips), n))
        for i, c in enumerate(clips):
            x[i, n - int(0.2 * SR) - len(c) : n - int(0.2 * SR)] += c * 32767
        emb = self.f.embed_clips(np.clip(x, -32768, 32767).astype(np.int16), batch_size=64)
        return [np.stack([e[k : k + 16] for k in range(e.shape[0] - 15)]) for e in emb]

    def score(self, w: np.ndarray) -> float:
        return max(float(self.s.run(None, {self.inp: v[None].astype(np.float32)})[0].reshape(-1)[0]) for v in w)

    def run(self, items: list) -> dict:
        """Note score et ressemblance dans chaque élément ; rend le seuil de ressemblance calibré (ou None)."""
        unit = lambda a: a.reshape(len(a), -1) / (np.linalg.norm(a.reshape(len(a), -1), axis=1, keepdims=True) + 1e-9)
        refs = [i for i in items if i["kind"] == "guide"]
        ref = unit(np.stack([w[-1] for w in self.windows([trim(i["m"]["x"]) for i in refs])])) if len(refs) >= MIN_REFS else None
        thr = None
        if ref is not None:
            s = ref @ ref.T
            np.fill_diagonal(s, -1)
            thr = float(np.percentile(s.max(axis=1), 10))  # 9 guidés sur 10 se ressemblent au moins autant entre eux
        todo = [i for i in items if i["kind"] in ("appris_pos", "appris_neg", "faux_declenchement", "quasi_declenchement")]
        for k in range(0, len(todo), 256):
            batch = todo[k : k + 256]
            for i, w in zip(batch, self.windows([i["m"]["x"] for i in batch])):
                i["m"]["score"] = round(self.score(w), 4)
                if ref is not None:
                    i["m"]["ressemblance"] = round(float((unit(w) @ ref.T).max()), 4)
        return {"seuil_ressemblance": thr, "references": len(refs)}


def read_list(path: Path) -> set:
    if not path.exists():
        return set()
    return {l.strip().replace("\\", "/") for l in path.read_text(encoding="utf-8").splitlines() if l.strip() and not l.startswith("#")}


def clean(rec: Path, ww: Path, out: Path, model: str, split_path: Path, probe: Probe | None = None) -> dict:
    rec, out = rec.resolve(), out.resolve()
    if out == rec or rec in out.parents or out in rec.parents:
        raise SystemExit(f"--out ({out}) ne doit ni contenir tes enregistrements ni être dedans ({rec}).")
    if not rec.is_dir():
        raise SystemExit(f"Dossier d'enregistrements introuvable : {rec}")
    max_speech = MODELS[model][2]
    items = collect(rec, model)
    seen = {}
    for i in items:
        i["mtime"] = i["path"].stat().st_mtime
        i["m"] = analyse(i["path"])
        i["decision"], i["raison"] = decide(i["m"], i["kind"], max_speech)
        fp = i["empreinte"] = i["m"].get("empreinte")
        if i["decision"] != "rejet" and fp:
            if fp in seen:  # le premier (guidés d'abord) l'emporte
                i["decision"], i["raison"] = "rejet", f"doublon de {seen[fp]}"
            else:
                seen[fp] = i["source"]

    info = {"seuil_ressemblance": None, "references": 0, "modele": None}
    if probe is not None:
        info.update(probe.run([i for i in items if i["decision"] == "garde"]), modele=True)
        for i in items:
            if i["decision"] == "garde":
                r = decide_probe(i["kind"], i["m"].get("score"), i["m"].get("ressemblance"), info["seuil_ressemblance"])
                if r:
                    i["decision"], i["raison"] = "quarantaine", r

    keep, drop = read_list(out / "garder.txt"), read_list(out / "exclure.txt")
    for i in items:
        names = {i["source"], i["path"].name}
        if names & drop:
            i["decision"], i["raison"] = "rejet", "exclu_par_toi"
        elif names & keep and i["decision"] == "quarantaine":
            i["decision"], i["raison"] = "garde", f"garde_par_toi ({i['raison']})"

    sessions(items)
    guided = [i for i in items if i["kind"] == "guide" and i["decision"] == "garde"]
    frozen = json.loads(split_path.read_text(encoding="utf-8")) if split_path.exists() else {}
    legacy = legacy_split(ww, model) if not frozen else (set(), set())
    new = assign_split(guided, frozen, *legacy, model)
    frozen.update(new)
    split_path.parent.mkdir(parents=True, exist_ok=True)
    split_path.write_text(json.dumps(frozen, indent=1, ensure_ascii=False), encoding="utf-8")
    for i in guided:
        i["split"] = frozen[i["m"]["empreinte"]]["split"]
    for i in items:
        if ROLE[i["kind"]] == "pos" and i["decision"] == "garde" and i["kind"] != "guide":
            i["split"] = "train"  # positifs appris : jamais en test

    # Copies : seuls nos dossiers de travail sont vidés, jamais les enregistrements.
    for d in ("recordings", "quarantaine"):
        shutil.rmtree(out / d, ignore_errors=True)
    lines = []
    for i in items:
        if i["decision"] == "rejet":
            continue
        if i["decision"] == "quarantaine":
            dst = out / "quarantaine" / i["raison"] / i["source"].replace("/", "__")
        else:
            sub = "test" if i.get("split") == "test" else DEST[i["kind"]]
            dst = out / "recordings" / sub / i["path"].name
            lines.append(f"{dst.relative_to(out).as_posix()} {i['m']['empreinte']}")
        dst.parent.mkdir(parents=True, exist_ok=True)
        if i["m"]["natif"]:
            shutil.copy2(i["path"], dst)
        else:
            sf.write(str(dst), i["m"]["x"], SR, subtype="PCM_16")
    data_hash = hashlib.sha1("\n".join(sorted(lines)).encode()).hexdigest()[:16]
    (out / "data_hash.txt").write_text(data_hash + "\n", encoding="utf-8")

    report = summarize(items, info, new, out, split_path, rec, data_hash)
    (out / "nettoyage.json").write_text(json.dumps(report, indent=1, ensure_ascii=False), encoding="utf-8")
    (out / "nettoyage.md").write_text(to_markdown(report), encoding="utf-8")
    return report


def summarize(items, info, new, out, split_path, rec, data_hash) -> dict:
    keys = ("duree", "pic", "rms_db", "sature", "dyn_db", "parole_s", "etendue_s", "score", "ressemblance", "sr", "canaux", "erreur")
    files = [{"source": i["source"], "type": i["kind"], "decision": i["decision"], "raison": i["raison"],
              "split": i.get("split"), "prise": i.get("prise"), "empreinte": i["m"].get("empreinte"),
              **{k: (round(i["m"][k], 4) if isinstance(i["m"][k], float) else i["m"][k]) for k in keys if k in i["m"]}}
             for i in items]
    counts = {}
    for f in files:
        c = counts.setdefault(f["type"], {"total": 0, "garde": 0, "rejet": 0, "quarantaine": 0})
        c["total"] += 1
        c[f["decision"]] += 1
    reasons = {}
    for f in files:
        if f["decision"] != "garde":
            reasons[f"{f['decision']}:{f['raison'].split(' de ')[0]}"] = reasons.get(f"{f['decision']}:{f['raison'].split(' de ')[0]}", 0) + 1
    guided = [f for f in files if f["type"] == "guide" and f["decision"] == "garde"]
    return {"date": time.strftime("%Y-%m-%d %H:%M"), "enregistrements": str(rec), "sortie": str(out),
            "decoupage": str(split_path), "data_hash": data_hash, "modele_actuel": info,
            "compte": counts, "raisons": reasons,
            "guides_train": sum(f["split"] == "train" for f in guided), "guides_test": sum(f["split"] == "test" for f in guided),
            "nouveaux_dans_decoupage": len(new),
            "appris_pos_train": sum(f["type"] == "appris_pos" and f["decision"] == "garde" for f in files),
            "quarantaine": [f for f in files if f["decision"] == "quarantaine"], "fichiers": files}


def to_markdown(r: dict) -> str:
    md = [f"## Données ({r['date']})", "", f"Source : `{r['enregistrements']}` (jamais modifiée). Copies propres : `{r['sortie']}/recordings`.", "",
          "| type | total | gardés | rejetés | quarantaine |", "|---|---|---|---|---|"]
    md += [f"| {k} | {c['total']} | {c['garde']} | {c['rejet']} | {c['quarantaine']} |" for k, c in r["compte"].items()]
    md += ["", f"Voix guidée : **{r['guides_train']} en entraînement, {r['guides_test']} en test** (découpage figé `{r['decoupage']}`, "
           f"{r['nouveaux_dans_decoupage']} fichiers nouveaux ce passage). Positifs appris : {r['appris_pos_train']}, entraînement seulement.", ""]
    p = r["modele_actuel"]
    md.append("Contrôle par le modèle actuel : " + ("non fait (modèle absent ou --score-model non donné)." if not p["modele"] else
              f"fait ; ressemblance calibrée sur {p['references']} guidés, seuil {p['seuil_ressemblance']}."))
    if r["raisons"]:
        md += ["", "Écartés, par raison :", ""] + [f"- {k} : {v}" for k, v in sorted(r["raisons"].items())]
    if r["quarantaine"]:
        md += ["", f"**Quarantaine** (copies dans `{r['sortie']}/quarantaine/`, à écouter) : pour en garder un, ajoute son nom à "
               f"`{r['sortie']}/garder.txt` ; pour l'écarter pour de bon, à `exclure.txt`. Puis relance.", ""]
        md += [f"- `{q['source']}` : {q['raison']}" + (f" (score {q['score']}, ressemblance {q.get('ressemblance')})" if "score" in q else "")
               for q in r["quarantaine"]]
    return "\n".join(md) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True)
    ap.add_argument("--recordings", type=Path, required=True)
    ap.add_argument("--model", choices=sorted(MODELS), default="dis_iris")
    ap.add_argument("--out", type=Path, default=None)
    ap.add_argument("--split", type=Path, default=None, help="découpage figé (défaut $WW/split_<model>.json)")
    ap.add_argument("--score-model", type=Path, default=None, help="modèle actuel, pour les contrôles par score")
    a = ap.parse_args()
    out = a.out or a.ww / "clean" / a.model
    probe = Probe(a.score_model) if a.score_model and a.score_model.exists() else None
    r = clean(a.recordings, a.ww, out, a.model, a.split or a.ww / f"split_{a.model}.json", probe)
    print(to_markdown(r))


if __name__ == "__main__":
    main()
