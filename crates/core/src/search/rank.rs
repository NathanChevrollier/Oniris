use super::index::Entry;
use super::{FileIndex, Item, ItemKind, UsageMap};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Bonus d'usage = `FRECENCY_WEIGHT * ln(1 + frecency)` : le logarithme borne l'effet d'un usage
/// intensif (il ne peut pas écraser le score flou) ; un usage récent unique vaut ~28 points,
/// 10 usages récents ~96, 100 ~185. Un score nucleo de nom (poids 2 inclus) vaut quelques centaines.
pub const FRECENCY_WEIGHT: f64 = 40.0;
const HALF_LIFE_MS: f64 = 7.0 * 86_400_000.0;
/// Poids du score sur le nom face au chemin complet (poids 1).
const NAME_WEIGHT: u32 = 2;
/// Bonus quand le nom commence par la requête (insensible à la casse).
const PREFIX_BONUS: u32 = 100;
/// Sous ce nombre d'entrées par fil, le parallélisme ne rapporte rien.
const MIN_CHUNK: usize = 20_000;
const SEPS: [char; 2] = ['/', '\\'];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Hit {
    pub item: Item,
    pub score: u32,
}

/// Score d'usage amorti (demi-vie de 7 jours) : `score` tel qu'il était à `last_ms`, ramené à `now`.
pub fn frecency(score: f64, last_ms: i64, now: i64) -> f64 {
    let age = (now - last_ms).max(0) as f64;
    score * 0.5f64.powf(age / HALF_LIFE_MS)
}

fn bonus_of(score: f64, last_ms: i64, now: i64) -> f32 {
    (FRECENCY_WEIGHT * frecency(score, last_ms, now).ln_1p()) as f32
}

fn file_item(e: &Entry) -> Item {
    let dir = &e.path[..e.name_start as usize];
    Item {
        kind: if e.is_dir { ItemKind::Dir } else { ItemKind::File },
        key: e.path.to_string(),
        title: e.name().to_string(),
        subtitle: dir.trim_end_matches(SEPS).to_string(),
    }
}

pub fn query(index: &FileIndex, extra: &[&[Item]], usage: &UsageMap, q: &str, limit: usize) -> Vec<Hit> {
    query_at(index, extra, usage, q, limit, crate::time_util::now_ms())
}

/// Candidat classé : `idx` < `index.len()` désigne une entrée de l'index, sinon un item extra.
type Heap = BinaryHeap<Reverse<(u32, Reverse<usize>)>>;

fn push(h: &mut Heap, limit: usize, score: u32, idx: usize) {
    let k = Reverse((score, Reverse(idx)));
    if h.len() < limit {
        h.push(k);
    } else if h.peek().is_some_and(|min| k < *min) {
        // `Reverse` inverse l'ordre : k < min signifie « meilleur que le pire retenu ».
        h.pop();
        h.push(k);
    }
}

/// `sources` : items hors index, en tranches (une par source) pour ne jamais les recopier à chaque frappe.
pub fn query_at(index: &FileIndex, sources: &[&[Item]], usage: &UsageMap, q: &str, limit: usize, now: i64) -> Vec<Hit> {
    let extra: Vec<&Item> = sources.iter().flat_map(|s| s.iter()).collect();
    let q = q.trim();
    if limit == 0 {
        return Vec::new();
    }
    if q.is_empty() {
        return recent(&extra, usage, limit, now);
    }
    // Mots négatifs (!foo) : jugés sur le chemin complet (ils excluent). Mots positifs : le reste.
    let (neg_words, pos_words): (Vec<&str>, Vec<&str>) = q.split_whitespace().partition(|w| w.starts_with('!'));
    let pos_q = pos_words.join(" ");
    let pattern = Pattern::parse(&pos_q, CaseMatching::Smart, Normalization::Smart);
    let neg = (!neg_words.is_empty())
        .then(|| Pattern::parse(&neg_words.join(" "), CaseMatching::Smart, Normalization::Smart));
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buf = Vec::new();
    let mut heap = Heap::with_capacity(limit + 1);
    // Bonus fichiers/dossiers précalculés, cherchés par &str : aucune allocation dans le scan.
    let file_bonus: HashMap<&str, f32> = usage
        .iter()
        .filter(|((k, _), _)| matches!(k, ItemKind::File | ItemKind::Dir))
        .map(|((_, key), &(s, t))| (key.as_str(), bonus_of(s, t, now)))
        .collect();
    let extra_bonus = |kind: ItemKind, key: &str| -> u32 {
        usage.get(&(kind, key.to_string())).map_or(0, |&(s, t)| bonus_of(s, t, now) as u32)
    };
    let prefix = |name: &str| -> u32 {
        // Requête multi-mots : pas de notion de préfixe unique.
        let ok = !pos_q.is_empty() && !pos_q.contains(' ') && name.get(..pos_q.len()).is_some_and(|p| p.eq_ignore_ascii_case(&pos_q));
        if ok { PREFIX_BONUS } else { 0 }
    };
    // Atome ancré (^foo) : le chemin commence par le lecteur, il ne dit rien du nom, on note les deux.
    let independent = pos_words.iter().any(|w| w.starts_with('^'));
    // Un Matcher par fil (jamais partagé) ; l'index est découpé en tranches.
    let scan = |off: usize, chunk: &[Entry]| -> Heap {
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let mut buf = Vec::new();
        let mut heap = Heap::with_capacity(limit + 1);
        for (i, e) in chunk.iter().enumerate() {
            let name = e.name();
            if let Some(n) = &neg {
                if n.score(Utf32Str::new(&e.path, &mut buf), &mut matcher).is_none() {
                    continue;
                }
            }
            let path_score = pattern.score(Utf32Str::new(&e.path, &mut buf), &mut matcher);
            // Sans atome ancré/négatif, le nom est un suffixe du chemin : chemin raté => nom raté.
            if path_score.is_none() && !independent {
                continue;
            }
            let name_score = pattern.score(Utf32Str::new(name, &mut buf), &mut matcher);
            if path_score.is_none() && name_score.is_none() {
                continue;
            }
            let total = name_score.map_or(0, |s| s * NAME_WEIGHT + prefix(name))
                + path_score.unwrap_or(0)
                + file_bonus.get(&*e.path).map_or(0, |&b| b as u32);
            push(&mut heap, limit, total, off + i);
        }
        heap
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(index.entries.len() / MIN_CHUNK + 1);
    let chunk_len = index.entries.len().div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        let hs: Vec<_> = index
            .entries
            .chunks(chunk_len)
            .enumerate()
            .map(|(n, c)| sc.spawn(move || scan(n * chunk_len, c)))
            .collect();
        for h in hs {
            for Reverse((s, Reverse(i))) in h.join().expect("scan de recherche") {
                push(&mut heap, limit, s, i);
            }
        }
    });
    let base = index.entries.len();
    for (j, it) in extra.iter().enumerate() {
        let t = pattern.score(Utf32Str::new(&it.title, &mut buf), &mut matcher);
        let s = pattern.score(Utf32Str::new(&it.subtitle, &mut buf), &mut matcher);
        if t.is_none() && s.is_none() {
            continue;
        }
        if let Some(n) = &neg {
            if [&it.title, &it.subtitle].iter().any(|f| n.score(Utf32Str::new(f, &mut buf), &mut matcher).is_none()) {
                continue;
            }
        }
        let total = t.map_or(0, |t| t * NAME_WEIGHT + prefix(&it.title)) + s.unwrap_or(0) + extra_bonus(it.kind, &it.key);
        push(&mut heap, limit, total, base + j);
    }
    let mut best: Vec<_> = heap.into_iter().map(|Reverse((s, Reverse(i)))| (s, i)).collect();
    best.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    best.into_iter()
        .map(|(score, i)| Hit {
            item: if i < base { file_item(&index.entries[i]) } else { extra[i - base].clone() },
            score,
        })
        .collect()
}

/// Requête vide : éléments d'usage classés par le même score que celui renvoyé (bonus de fréquence).
/// Les fichiers/dossiers viennent de la table d'usage telle quelle : leur existence n'est PAS vérifiée.
fn recent(extra: &[&Item], usage: &UsageMap, limit: usize, now: i64) -> Vec<Hit> {
    let known: HashMap<(ItemKind, &str), &Item> = extra.iter().map(|&i| ((i.kind, i.key.as_str()), i)).collect();
    let mut v: Vec<_> = usage.iter().map(|(k, &(s, t))| (k, bonus_of(s, t, now) as u32)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.into_iter()
        .filter_map(|((kind, key), score)| {
            let item = match kind {
                ItemKind::File | ItemKind::Dir => {
                    let name_start = key.rfind(SEPS).map_or(0, |i| i + 1);
                    Item { kind: *kind, key: key.clone(), title: key[name_start..].to_string(), subtitle: key[..name_start].trim_end_matches(SEPS).to_string() }
                }
                _ => (*known.get(&(*kind, key.as_str()))?).clone(),
            };
            Some(Hit { item, score })
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::ItemKind;

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_000 * DAY;

    fn idx(paths: &[(&str, bool)]) -> FileIndex {
        let mut i = FileIndex::new();
        for (p, d) in paths {
            i.insert(p, *d);
        }
        i
    }
    fn keys(h: &[Hit]) -> Vec<&str> {
        h.iter().map(|h| h.item.key.as_str()).collect()
    }

    #[test]
    fn nom_exact_dossier_avant_fichier_dans_dossier_homonyme() {
        let i = idx(&[
            ("C:\\Users\\n\\Projets\\oniris-old\\readme.md", false),
            ("C:\\Users\\n\\Projets\\Oniris", true),
            ("C:\\Users\\n\\Projets\\archive\\oniris-old.md", false),
        ]);
        let h = query_at(&i, &[], &UsageMap::new(), "oniris", 10, NOW);
        assert_eq!(keys(&h)[0], "C:\\Users\\n\\Projets\\Oniris");
        assert_eq!(h[0].item.kind, ItemKind::Dir);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn negation_exclut_par_chemin() {
        let i = idx(&[
            ("C:\\Users\\n\\Projets\\Oniris", true),
            ("C:\\Users\\n\\Projets\\archive\\oniris-old.md", false),
        ]);
        let h = query_at(&i, &[], &UsageMap::new(), "oniris !archive", 10, NOW);
        assert_eq!(keys(&h), ["C:\\Users\\n\\Projets\\Oniris"]);
    }

    #[test]
    fn atome_ancre_trouve_le_nom() {
        let i = idx(&[("C:\\Users\\n\\Projets\\Oniris", true), ("C:\\Users\\n\\Projets\\vieux-oniris", true)]);
        let h = query_at(&i, &[], &UsageMap::new(), "^oniris", 10, NOW);
        assert_eq!(keys(&h), ["C:\\Users\\n\\Projets\\Oniris"]);
    }

    #[test]
    fn usage_ancien_massif_ne_depasse_pas_usage_recent_modeste() {
        let key = "C:\\a".to_string();
        let mut vieux = UsageMap::new();
        for _ in 0..500 {
            crate::search::bump(&mut vieux, ItemKind::File, &key, NOW - 365 * DAY);
        }
        crate::search::bump(&mut vieux, ItemKind::File, &key, NOW);
        let mut recent = UsageMap::new();
        for d in [6, 5, 4, 3, 2] {
            crate::search::bump(&mut recent, ItemKind::File, &key, NOW - d * DAY);
        }
        let f = |m: &UsageMap| {
            let &(s, t) = &m[&(ItemKind::File, key.clone())];
            bonus_of(s, t, NOW)
        };
        assert!(f(&vieux) < f(&recent), "{} vs {}", f(&vieux), f(&recent));
    }

    #[test]
    fn mots_dans_le_desordre() {
        let i = idx(&[
            ("C:\\Oniris\\docs\\plans\\2026-11-03-oniris-01-fondations.md", false),
            ("C:\\Oniris\\docs\\specs\\autre.md", false),
        ]);
        let h = query_at(&i, &[], &UsageMap::new(), "plan fond", 10, NOW);
        assert_eq!(h.len(), 1);
        assert!(h[0].item.key.ends_with("2026-11-03-oniris-01-fondations.md"));
        assert_eq!(h[0].item.title, "2026-11-03-oniris-01-fondations.md");
    }

    #[test]
    fn aucune_correspondance_et_limite() {
        let i = idx(&[("C:\\a\\foo", true), ("C:\\a\\foobar", true), ("C:\\a\\foobaz", true)]);
        assert!(query_at(&i, &[], &UsageMap::new(), "zzzq", 10, NOW).is_empty());
        assert_eq!(query_at(&i, &[], &UsageMap::new(), "foo", 2, NOW).len(), 2);
        assert!(query_at(&i, &[], &UsageMap::new(), "foo", 0, NOW).is_empty());
    }

    #[test]
    fn frecency_decroit_demi_vie_7_jours() {
        let f0 = frecency(8.0, NOW, NOW);
        assert!((f0 - 8.0).abs() < 1e-9);
        assert!((frecency(8.0, NOW - 7 * DAY, NOW) - 4.0).abs() < 1e-9);
        assert!(frecency(8.0, NOW - 30 * DAY, NOW) < 0.5);
        // dernier usage dans le futur (horloge) : pas d'amplification
        assert!((frecency(8.0, NOW + DAY, NOW) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn element_frequent_devant_score_flou_legerement_meilleur() {
        let i = idx(&[("C:\\x\\notes", true), ("C:\\x\\note-s", true)]);
        let mut u = UsageMap::new();
        let base = query_at(&i, &[], &u, "notes", 10, NOW);
        assert_eq!(keys(&base)[0], "C:\\x\\notes");
        u.insert((ItemKind::Dir, "C:\\x\\note-s".into()), (30.0, NOW));
        let h = query_at(&i, &[], &u, "nts", 10, NOW);
        assert_eq!(h.len(), 2);
        let h = query_at(&i, &[], &u, "notes", 10, NOW);
        assert_eq!(keys(&h)[0], "C:\\x\\note-s");
        // plus ancien : la fréquence a décru, le meilleur score flou revient devant
        let mut old = UsageMap::new();
        old.insert((ItemKind::Dir, "C:\\x\\note-s".into()), (30.0, NOW - 60 * DAY));
        assert_eq!(keys(&query_at(&i, &[], &old, "notes", 10, NOW))[0], "C:\\x\\notes");
    }

    #[test]
    fn requete_vide_recents_par_usage() {
        let i = idx(&[("C:\\a\\vieux", false), ("C:\\a\\recent", false)]);
        let extra = vec![Item { kind: ItemKind::App, key: "app1".into(), title: "Calc".into(), subtitle: String::new() }];
        let mut u = UsageMap::new();
        u.insert((ItemKind::File, "C:\\a\\vieux".into()), (1.0, NOW - 5 * DAY));
        u.insert((ItemKind::File, "C:\\a\\recent".into()), (1.0, NOW - DAY));
        u.insert((ItemKind::App, "app1".into()), (1.0, NOW));
        u.insert((ItemKind::App, "disparu".into()), (1.0, NOW)); // inconnu : ignoré
        let h = query_at(&i, &[&extra], &u, "  ", 10, NOW);
        assert_eq!(keys(&h), ["app1", "C:\\a\\recent", "C:\\a\\vieux"]);
        assert_eq!(query_at(&i, &[&extra], &u, "", 2, NOW).len(), 2);
    }

    #[test]
    fn items_extra_cherches_sur_titre_et_sous_titre() {
        let extra = vec![Item { kind: ItemKind::Project, key: "p1".into(), title: "Zenytt".into(), subtitle: "gestion de VPS".into() }];
        let u = UsageMap::new();
        let e = FileIndex::new();
        assert_eq!(query_at(&e, &[&extra], &u, "zen", 5, NOW).len(), 1);
        assert_eq!(query_at(&e, &[&extra], &u, "vps", 5, NOW).len(), 1);
        assert!(query_at(&e, &[&extra], &u, "xyz", 5, NOW).is_empty());
    }

    /// Lancer : `cargo test -p oniris-core --release search_perf -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn search_perf() {
        perf(false);
    }

    /// Même mesure avec 5000 usages pointant dans l'index.
    #[test]
    #[ignore]
    fn search_perf_usage() {
        perf(true);
    }

    fn perf(with_usage: bool) {
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut rnd = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let roots = ["Users\\natha\\Documents", "Users\\natha\\Desktop\\Projets", "Program Files", "Users\\natha\\AppData\\Local"];
        let dirs = ["src", "components", "assets", "node_modules", "docs", "tests", "build", "lib", "client", "server", "images", "config"];
        let words = ["rapport", "facture", "index", "main", "readme", "budget", "photo", "notes", "plan", "setup", "oniris", "widget", "style", "utils", "model", "vacances"];
        let exts = ["rs", "ts", "tsx", "md", "png", "json", "pdf", "docx", "txt", "css"];
        let mut i = FileIndex::new();
        for n in 0..300_000 {
            let mut p = format!("C:\\{}", roots[rnd(roots.len())]);
            for _ in 0..2 + rnd(5) {
                p.push('\\');
                p.push_str(dirs[rnd(dirs.len())]);
                if rnd(3) == 0 {
                    p.push_str(&format!("-{}", rnd(100)));
                }
            }
            p.push('\\');
            p.push_str(&format!("{}_{}_{}.{}", words[rnd(words.len())], words[rnd(words.len())], n, exts[rnd(exts.len())]));
            i.insert(&p, false);
        }
        assert_eq!(i.len(), 300_000);
        let mut usage = UsageMap::new();
        if with_usage {
            for e in i.entries.iter().step_by(60).take(5000) {
                usage.insert((ItemKind::File, e.path.to_string()), (3.0, NOW - DAY));
            }
            assert_eq!(usage.len(), 5000);
        }
        let qs = ["rap", "fac", "onr", "ind", "bud", "pho", "wid", "not", "set", "vac"];
        let mut times = Vec::new();
        for k in 0..20 {
            let t = std::time::Instant::now();
            let h = query_at(&i, &[], &usage, qs[k % qs.len()], 20, NOW);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            assert!(!h.is_empty());
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = times[10];
        println!("search_perf (usage={with_usage}) : 300000 entrées, médiane {median:.1} ms, min {:.1}, max {:.1}", times[0], times[19]);
        assert!(median < 50.0, "médiane {median} ms");
    }
}
