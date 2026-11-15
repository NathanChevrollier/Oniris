"""Garde-fou du réentraînement : mesure le modèle livré et chaque candidat EXACTEMENT de la même façon, ne remplace
le modèle livré que si un candidat est au moins aussi bon, puis écrit le rapport final.

  python gate.py --ww ~/oniris-wakeword --repo <dépôt> --model dis_iris --sweep-json output/sweep_dis_iris.json \
      [--clean-report clean/dis_iris/nettoyage.json] [--long FICHIERS_SANS_IRIS ...] [--promote]

Mêmes mesures pour tous (modèle livré compris) :
- rappel sur TA voix : tes enregistrements guidés de test (own_*.wav), chacun mélangé UNE fois à ton ambiance, le même
  mélange pour tous les modèles, lu en flux comme dans Oniris ;
- rappel synthétique : output/<model>/positive_features_test.npy ;
- faux déclenchements par heure : 11,3 h de validation d'openWakeWord (+ --long), en flux, 2 s de pause après chaque.
Point de fonctionnement d'un modèle pour un objectif T (0,5 puis 0,1 par heure) : le seuil le plus bas qui tient T.
Promotion (pour T = 0,5 ET T = 0,1) : le candidat tient T si le modèle livré le tient ; sinon il ne fait pas plus de
faux déclenchements ; et son rappel sur ta voix n'est pas plus bas. Le meilleur candidat admis remplace le modèle
livré (sauvegarde horodatée d'abord, make_fixture.py ensuite, retour arrière si la fixture échoue).
"""
import argparse
import json
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np

KIT = Path(__file__).resolve().parent
TARGETS = (0.5, 0.1)
GOAL_RECALL = 0.9


def point(curve: dict, target: float) -> dict:
    """Seuil le plus bas qui tient `target` faux/h ; à défaut, le moins de faux déclenchements (seuil le plus haut)."""
    ts = sorted(curve, key=float)
    ok = [t for t in ts if curve[t]["faux_par_heure"] <= target]
    t = ok[0] if ok else min(ts, key=lambda t: (curve[t]["faux_par_heure"], -float(t)))
    return dict(curve[t], seuil=float(t), tenu=bool(ok))


def recall(p: dict):
    """Rappel sur ta voix, ou synthétique s'il n'y a aucun enregistrement de test."""
    return p["rappel_ta_voix"] if p["rappel_ta_voix"] is not None else p["rappel_synth"]


def admissible(cand: dict, old: dict | None) -> list:
    """Raisons de refus d'un candidat face au modèle livré (liste vide : au moins aussi bon)."""
    if old is None:
        return []
    why = []
    for t in TARGETS:
        c, o = point(cand, t), point(old, t)
        if o["tenu"] and not c["tenu"]:
            why.append(f"ne tient pas {t} faux/h (le modèle livré oui)")
        elif not (o["tenu"] and c["tenu"]) and c["faux_par_heure"] > o["faux_par_heure"]:
            why.append(f"objectif {t} : {c['faux_par_heure']} faux/h contre {o['faux_par_heure']}")
        if recall(c) < recall(o):
            why.append(f"objectif {t} : rappel ta voix {recall(c):.0%} contre {recall(o):.0%}")
    return why


def choose(old: dict | None, cands: dict) -> tuple:
    """(nom du gagnant ou None, {nom: raisons de refus})."""
    refus = {n: admissible(c, old) for n, c in cands.items()}
    ok = [n for n in cands if not refus[n]]
    key = lambda n: tuple(x for t in (0.1, 0.5) for x in (recall(point(cands[n], t)), -point(cands[n], t)["faux_par_heure"])) + \
        (point(cands[n], 0.5)["rappel_synth"],)
    return (max(ok, key=key) if ok else None), refus


def advised_threshold(curve: dict) -> tuple:
    """(seuil, pourquoi) : le plus bas qui tient l'objectif du plan (≤ 0,1 faux/h et ≥ 90 % ta voix), sinon le point 0,1."""
    for t in sorted(curve, key=float):
        r = curve[t]
        if r["faux_par_heure"] <= 0.1 and (recall(r) or 0) >= GOAL_RECALL:
            return float(t), "tient l'objectif du plan (≤ 0,1 faux/h et ≥ 90 % de ta voix)"
    p = point(curve, 0.1)
    if p["tenu"]:
        return p["seuil"], "tient 0,1 faux/h, mais rappel ta voix sous 90 %"
    p = point(curve, 0.5)
    return p["seuil"], "aucun seuil ne tient 0,1 faux/h : seuil le plus bas qui tient 0,5" + ("" if p["tenu"] else " (non tenu non plus)")


def wake_threshold_line(repo: Path) -> tuple:
    """(fichier relatif, ligne, valeur) de la constante WAKE_THRESHOLD côté Rust, ou None."""
    for f in sorted((repo / "src-tauri" / "src").rglob("*.rs")):
        for n, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
            m = re.match(r"\s*pub const WAKE_THRESHOLD: f32 = ([0-9.]+);", line)
            if m:
                return f.relative_to(repo).as_posix(), n, float(m.group(1))
    return None


# --- mesures (venv WSL seulement) ---

def own_mixes(ww: Path, model: str) -> list:
    """Chaque clip de test de ta voix sur ton ambiance, tiré une fois (graine fixe) : le même pour tous les modèles."""
    import evaluate

    evaluate.load_bed(ww)
    evaluate._rng = np.random.default_rng(8)
    out = []
    for f in sorted((ww / "output" / model / "positive_test").glob("own_*.wav")):
        clip = evaluate.load(f).astype(np.float32)
        pcm = evaluate.bed(evaluate.SR + len(clip) + evaluate.SR)
        pcm[evaluate.SR : evaluate.SR + len(clip)] += clip
        out.append((f.name, np.clip(pcm, -32768, 32767).astype(np.int16)))
    return out


def measure(path: Path, mixes: list, pos: np.ndarray, val: np.ndarray, longs: list) -> dict:
    import evaluate
    import sweep
    from openwakeword.model import Model

    m = Model(wakeword_models=[str(path)], inference_framework="onnx")
    evaluate.NAME = path.stem
    own = np.array([float(evaluate.scores(m, pcm).max()) for _, pcm in mixes])
    synth = sweep.feature_scores(path, pos)
    fph = sweep.val_fph(path, val)
    long_scores = [evaluate.scores(m, pcm) for pcm in longs]
    long_h = sum(len(p) for p in longs) / evaluate.SR / 3600
    curve = {}
    for t in sweep.THRESHOLDS:
        n = fph[t] * sweep.VAL_HOURS + sum(evaluate.activations(s, t) for s in long_scores)
        curve[str(t)] = {"faux_par_heure": round(n / (sweep.VAL_HOURS + long_h), 3),
                         "rappel_synth": round(float(np.mean(synth >= t)), 3),
                         "rappel_ta_voix": round(float(np.mean(own >= t)), 3) if len(own) else None}
    return curve


# --- sauvegarde, promotion ---

def paths(repo: Path, model: str) -> dict:
    wake = repo / "src-tauri" / "resources" / "wake"
    return {"model": wake / f"{model}.onnx", "sums": wake / "SHA256SUMS", "fixture": repo / "src-tauri" / "tests" / "fixtures" / "wake" / model}


def backup(repo: Path, model: str, dst: Path) -> Path | None:
    p = paths(repo, model)
    if not p["model"].exists():
        return None
    dst.mkdir(parents=True, exist_ok=True)
    shutil.copy2(p["model"], dst / p["model"].name)
    if p["sums"].exists():
        shutil.copy2(p["sums"], dst / "SHA256SUMS")
    if p["fixture"].exists():
        shutil.copytree(p["fixture"], dst / "fixture", dirs_exist_ok=True)
    return dst / p["model"].name


def restore(repo: Path, model: str, src: Path) -> None:
    p = paths(repo, model)
    shutil.copy2(src / p["model"].name, p["model"])
    if (src / "SHA256SUMS").exists():
        shutil.copy2(src / "SHA256SUMS", p["sums"])
    if (src / "fixture").exists():
        shutil.rmtree(p["fixture"], ignore_errors=True)
        shutil.copytree(src / "fixture", p["fixture"])


def table(rows: dict, t: float) -> list:
    md = [f"Objectif {t} faux/h :", "", "| modèle | seuil | faux/h | rappel ta voix | rappel synth | tenu |", "|---|---|---|---|---|---|"]
    for n, c in rows.items():
        p = point(c, t)
        v = "—" if p["rappel_ta_voix"] is None else f"{p['rappel_ta_voix']:.0%}"
        md.append(f"| {n} | {p['seuil']} | {p['faux_par_heure']} | {v} | {p['rappel_synth']:.0%} | {'oui' if p['tenu'] else 'non'} |")
    return md + [""]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ww", type=Path, required=True)
    ap.add_argument("--repo", type=Path, required=True)
    ap.add_argument("--model", default="dis_iris")
    ap.add_argument("--sweep-json", type=Path, default=None, help="candidats : variantes de ce fichier (défaut output/sweep_<model>.json)")
    ap.add_argument("--candidates", nargs="*", default=None, help="ou des .onnx donnés directement")
    ap.add_argument("--clean-report", type=Path, default=None)
    ap.add_argument("--long", nargs="*", default=[])
    ap.add_argument("--promote", action="store_true", help="sans lui : mesure et rapport seulement")
    ap.add_argument("--report", type=Path, default=None)
    a = ap.parse_args()
    sys.path.insert(0, str(KIT))
    import evaluate

    repo, out, stamp = a.repo.resolve(), a.ww / "output", time.strftime("%Y%m%d_%H%M%S")
    if a.candidates is not None:
        cands = {Path(c).stem: Path(c) for c in a.candidates}
    else:
        sj = json.loads((a.sweep_json or out / f"sweep_{a.model}.json").read_text(encoding="utf-8"))
        cands = {f"{a.model}_{r['variante']}": out / "sweep" / f"{a.model}_{r['variante']}.onnx" for r in sj["all"]}
    missing = [str(p) for p in cands.values() if not p.exists()]
    if missing:
        raise SystemExit(f"Candidats introuvables : {missing}")

    saved = backup(repo, a.model, out / "backups" / f"{a.model}_{stamp}")
    print(f"Sauvegarde du modèle livré : {saved.parent if saved else 'aucun modèle livré'}", flush=True)
    same = [n for n, p in cands.items() if saved and p.read_bytes() == saved.read_bytes()]
    for n in same:  # le modèle livré lui-même (variante reprise) : le « promouvoir » ne changerait rien
        print(f"  {n} : identique au modèle livré, ignoré", flush=True)
        del cands[n]
    if not cands:
        raise SystemExit("Aucun candidat différent du modèle livré.")

    feats = out / a.model
    pos = np.load(feats / "positive_features_test.npy")
    val = np.load(a.ww / "data" / "validation_set_features.npy", mmap_mode="r")
    val = np.asarray(val.reshape(-1, val.shape[-1]) if val.ndim > 2 else val, dtype=np.float32)
    mixes = own_mixes(a.ww, a.model)
    longs = [evaluate.load(f) for f in evaluate.files(a.long)]
    print(f"Jeu de test figé : {len(mixes)} clips de ta voix, {len(pos)} synthétiques, {11.3 + sum(map(len, longs)) / 16000 / 3600:.1f} h de négatifs", flush=True)

    curves = {}
    for name, p in ([("modèle livré", saved)] if saved else []) + list(cands.items()):
        t0 = time.time()
        curves[name] = measure(p, mixes, pos, val, longs)
        q = point(curves[name], 0.5)
        print(f"  {name} : seuil {q['seuil']}, {q['faux_par_heure']} faux/h, ta voix {recall(q):.0%} ({time.time() - t0:.0f} s)", flush=True)
    old = curves.get("modèle livré")
    winner, refus = choose(old, {n: curves[n] for n in cands})

    promoted = False
    if winner and not a.promote:
        decision = f"{winner} serait promu (mesure seule, --promote absent)"
    elif winner:
        shutil.copy2(cands[winner], out / f"{a.model}.onnx")
        r = subprocess.run([sys.executable, str(KIT / "make_fixture.py"), "--ww", str(a.ww), "--repo", str(repo), "--model", a.model])
        promoted = r.returncode == 0
        if not promoted and saved:
            restore(repo, a.model, saved.parent)
        decision = (f"**{winner} promu** dans src-tauri/resources/wake (+ SHA256SUMS et fixture)" if promoted
                    else f"{winner} retenu mais make_fixture.py a échoué : modèle livré restauré")
    else:
        decision = "**modèle livré conservé** : aucun candidat n'est au moins aussi bon"
    if not promoted and saved:
        shutil.copy2(saved, out / f"{a.model}.onnx")  # sweep.py y a copié sa gagnante : on remet celui qui est livré

    final = winner if promoted or not old else "modèle livré"
    thr, why = advised_threshold(curves[final]) if final else (None, "")
    loc = wake_threshold_line(repo)

    md = [f"# Réentraînement {a.model} — {time.strftime('%Y-%m-%d %H:%M')}", ""]
    if a.clean_report and a.clean_report.exists():
        import clean_data

        md += [clean_data.to_markdown(json.loads(a.clean_report.read_text(encoding="utf-8")))]
    md += ["## Scores (même test figé pour tous)", "",
           f"{len(mixes)} clips de ta voix (test guidé), {len(pos)} clips synthétiques, "
           f"{11.3 + sum(map(len, longs)) / 16000 / 3600:.1f} h de négatifs. Rappel sur ta voix : peu de clips, à lire avec prudence.", ""]
    for t in TARGETS:
        md += table(curves, t)
    md += ["## Décision", "", decision, ""]
    md += [f"- {n} : refusé ({'; '.join(r)})" for n, r in refus.items() if r]
    if saved:
        md += ["", f"Sauvegarde de l'ancien modèle : `{saved.parent}` (modèle, SHA256SUMS, fixture)."]
    md += ["", "## Seuil conseillé", ""]
    if thr is not None:
        md.append(f"**{thr}** pour {final} : {why}.")
        if loc:
            md.append(f"Actuel : `WAKE_THRESHOLD = {loc[2]}` dans `{loc[0]}`, ligne {loc[1]}. "
                      + ("Rien à changer." if abs(loc[2] - thr) < 1e-9 else "À changer à la main (le script ne touche pas au code Rust)."))
    md += ["", "## Prochaines actions", ""]
    if promoted:
        md += ["1. `cargo test` (fixture du détecteur), puis essai réel : mode apprentissage actif, une soirée normale.",
               "2. Ajuster WAKE_THRESHOLD si la valeur conseillée diffère, puis commit des modèles, SHA256SUMS et fixture.",
               "3. Retour arrière possible : recopier la sauvegarde ci-dessus dans src-tauri/resources/wake et la fixture."]
    else:
        md += ["1. Continuer le mode apprentissage (surtout « Faux réveil » et « Je vais dire Iris »), puis relancer.",
               "2. Si ta voix est sous 90 % : enregistrer une nouvelle prise guidée (record_samples.py --phrase dis_iris).",
               "3. Si trop de faux déclenchements : sessions listen_live.py en vie normale, puis relancer."]
    report = a.report or out / f"rapport_{a.model}_{stamp}.md"
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text("\n".join(md) + "\n", encoding="utf-8")
    (report.with_suffix(".json")).write_text(json.dumps({"decision": decision, "gagnant": winner, "refus": refus,
                                                         "seuil_conseille": thr, "courbes": curves}, indent=1, ensure_ascii=False), encoding="utf-8")
    print("\n".join(md[md.index("## Décision"):]))
    print(f"\nRapport : {report}")


if __name__ == "__main__":
    main()
