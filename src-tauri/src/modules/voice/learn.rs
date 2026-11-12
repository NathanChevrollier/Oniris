//! Mode apprentissage (réglage `voice_wake_learning`, désactivé par défaut) : seule exception à « l'audio d'avant la
//! détection n'est jamais écrit ». Un anneau de 6 s, en mémoire, garde l'audio que voit le détecteur ; des extraits
//! étiquetés (3 s au plus) autour d'un réveil, d'un presque-réveil ou d'un appui sur « Je vais dire Iris » sont écrits
//! dans `learn/{pos,neg}/`, pour réentraîner le modèle. Jamais transcrits ni envoyés.
//!
//! Horloge : `at_ms` est le temps d'audio poussé depuis la création du collecteur (`now_ms`), ce qui situe un
//! événement dans l'anneau ; elle s'arrête quand le micro est fermé.
use super::wake::{NEAR_MISS, WAKE_THRESHOLD};
use oniris_core::voice::audio::wav_16k_mono;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Un positif fait 1,6 s et finit 0,2 s après le meilleur score : openWakeWord tronque par la fin au-delà de 2 s.
pub const POS_BEFORE_MS: u64 = 1400;
pub const POS_AFTER_MS: u64 = 200;
pub const NEG_MS: u64 = 3000;
/// Presque-réveil suivi d'un réveil ou d'un push-to-talk dans ce délai : l'utilisateur a répété.
pub const RETRY_WINDOW_MS: u64 = 5000;
pub const EXPECT_WINDOW_MS: u64 = 5000;
pub const FALSE_WAKE_WINDOW_MS: u64 = 10_000;
pub const EXPECT_MIN_SCORE: f32 = 0.05;
pub const MAX_FILES: usize = 2000;
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;

/// Dossier des extraits (`None` : pas de dossier personnel).
pub fn learn_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("oniris-wakeword").join("recordings").join("learn"))
}

/// Les `*.wav` de `pos` et de `neg`, rien d'autre.
fn wavs(dir: &std::path::Path, sub: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav")).collect()
}

/// (positifs, négatifs) présents dans `dir`.
pub fn count_in(dir: &std::path::Path) -> (u32, u32) { (wavs(dir, "pos").len() as u32, wavs(dir, "neg").len() as u32) }

/// Supprime les extraits de `dir` et renvoie leur nombre.
pub fn clear_in(dir: &std::path::Path) -> u32 {
    ["pos", "neg"].iter().flat_map(|s| wavs(dir, s)).filter(|p| std::fs::remove_file(p).is_ok()).count() as u32
}

const PER_MS: u64 = 16;
const RING: usize = 6000 * PER_MS as usize;
/// En deçà, l'anneau ne tient pas assez d'audio : rien n'est écrit.
const MIN_CLIP: u64 = 1000 * PER_MS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label { Pos, Neg }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Score { at_ms: u64, score: f32 },
    Woke { at_ms: u64 },
    /// Fin de parole après le réveil (`Endpoint::End` ou `TooLong`).
    Command,
    NoSpeech,
    /// Échap entre le réveil et la fin de l'enregistrement.
    Cancelled,
    /// Push-to-talk (`Input::Start`).
    ManualStart { at_ms: u64 },
    ExpectWake { at_ms: u64 },
    FalseWake { at_ms: u64 },
}

#[derive(Default)]
struct Ring { buf: VecDeque<f32>, total: u64 }

impl Ring {
    fn now_ms(&self) -> u64 { self.total / PER_MS }

    /// Les `len_ms` finissant à `end_ms`, réduits à ce que l'anneau tient (sans remplissage) ; `None` sous 1 s.
    fn cut(&self, end_ms: u64, len_ms: u64) -> Option<Vec<f32>> {
        let base = self.total - self.buf.len() as u64;
        let end = (end_ms * PER_MS).min(self.total);
        let start = (end_ms.saturating_sub(len_ms) * PER_MS).max(base);
        if end < start + MIN_CLIP {
            return None;
        }
        Some(self.buf.range((start - base) as usize..(end - base) as usize).copied().collect())
    }
}

/// Un score et l'extrait positif autour, copié dès que l'audio des 200 ms suivantes est arrivé.
struct Peak { at_ms: u64, score: f32, clip: Option<Vec<f32>> }

impl Peak {
    fn new(at_ms: u64, score: f32) -> Self { Self { at_ms, score, clip: None } }
    fn fill(&mut self, ring: &Ring) {
        if self.clip.is_none() && ring.now_ms() >= self.at_ms + POS_AFTER_MS {
            self.clip = ring.cut(self.at_ms + POS_AFTER_MS, POS_BEFORE_MS + POS_AFTER_MS);
        }
    }
    /// L'extrait copié, ou ce que l'anneau tient encore.
    fn take(self, ring: &Ring) -> Option<Vec<f32>> {
        self.clip.or_else(|| ring.cut(self.at_ms + POS_AFTER_MS, POS_BEFORE_MS + POS_AFTER_MS))
    }
}

/// Réveil en attente : son extrait n'est rendu qu'après la fenêtre du bouton « faux réveil ».
struct Wake { peak: Peak, neg: Option<Vec<f32>>, label: Option<Label>, near: Option<Vec<f32>> }

/// Unicité des noms, même pour deux extraits de la même milliseconde.
static SEQ: AtomicU64 = AtomicU64::new(0);

pub struct Collector {
    dir: PathBuf,
    ring: Ring,
    near: Option<Peak>,
    /// Appui sur « Je vais dire Iris » et le meilleur score depuis.
    expect: Option<(u64, Option<Peak>)>,
    wake: Option<Wake>,
    /// Extraits décidés, rendus un par appel à `on`.
    out: VecDeque<(Label, Vec<f32>)>,
}

impl Collector {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, ring: Ring::default(), near: None, expect: None, wake: None, out: VecDeque::new() }
    }

    pub fn dir(&self) -> &PathBuf { &self.dir }

    pub fn now_ms(&self) -> u64 { self.ring.now_ms() }

    /// Micro fermé : l'audio d'avant (dont la fin d'une demande) ne doit jamais finir dans un extrait coupé plus tard.
    /// L'horloge continue : `cut` rend un extrait plus court, ou rien sous 1 s. Ce qui a été copié au réveil reste.
    pub fn gap(&mut self) { self.ring.buf.clear() }

    /// Abandonne tout ce qui attend d'être écrit (« Supprimer les extraits »).
    pub fn drop_pending(&mut self) {
        (self.near, self.expect, self.wake) = (None, None, None);
        self.out.clear();
    }

    pub fn push_audio(&mut self, samples_16k: &[f32]) {
        let r = &mut self.ring;
        r.buf.extend(samples_16k);
        r.total += samples_16k.len() as u64;
        let over = r.buf.len().saturating_sub(RING);
        r.buf.drain(..over);
        let peaks = [self.near.as_mut(), self.expect.as_mut().and_then(|e| e.1.as_mut()), self.wake.as_mut().map(|w| &mut w.peak)];
        for p in peaks.into_iter().flatten() {
            p.fill(&self.ring);
        }
    }

    /// Étiquetage (les boutons l'emportent) ; rend au plus un extrait à écrire, les suivants aux appels d'après.
    pub fn on(&mut self, e: Event) -> Option<(Label, Vec<f32>)> {
        match e {
            Event::Score { at_ms, score } => {
                if self.wake.as_ref().is_some_and(|w| at_ms >= w.peak.at_ms + FALSE_WAKE_WINDOW_MS) {
                    self.resolve();
                }
                if let Some((t, best)) = &mut self.expect {
                    if at_ms > *t + EXPECT_WINDOW_MS {
                        let best = best.take().filter(|b| b.score > EXPECT_MIN_SCORE);
                        if let Some(clip) = best.and_then(|b| b.take(&self.ring)) {
                            self.out.push_back((Label::Pos, clip));
                        }
                        self.expect = None;
                    } else if best.as_ref().is_none_or(|b| score > b.score) {
                        *best = Some(Peak::new(at_ms, score));
                    }
                }
                let stale = |n: &Peak| at_ms > n.at_ms + RETRY_WINDOW_MS;
                if (NEAR_MISS..WAKE_THRESHOLD).contains(&score) && self.near.as_ref().is_none_or(|n| score > n.score || stale(n)) {
                    self.near = Some(Peak::new(at_ms, score));
                }
            }
            Event::Woke { at_ms } => {
                self.resolve();
                // Un presque-réveil de moins de 1,6 s avant est la montée du même mot, pas un essai raté.
                let near = self.near.take().filter(|n| at_ms <= n.at_ms + RETRY_WINDOW_MS && n.at_ms + POS_BEFORE_MS + POS_AFTER_MS <= at_ms);
                let expected = self.expect.take().is_some_and(|(t, _)| at_ms <= t + EXPECT_WINDOW_MS);
                self.wake = Some(Wake {
                    peak: Peak::new(at_ms, 1.0),
                    neg: self.ring.cut(at_ms, NEG_MS),
                    label: expected.then_some(Label::Pos),
                    near: near.and_then(|n| n.take(&self.ring)),
                });
            }
            // Réveil puis silence : on ne sait pas (vrai « Dis Iris » hésitant ou faux réveil), rien n'est gardé ;
            // seul le bouton « Faux réveil » en fait un négatif (décision du 12/11).
            Event::NoSpeech => {}
            Event::Command | Event::Cancelled => {
                if let Some(w) = self.wake.as_mut().filter(|w| w.label.is_none()) {
                    w.label = Some(if e == Event::Command { Label::Pos } else { Label::Neg });
                }
            }
            Event::ManualStart { at_ms } => {
                let near = self.near.take().filter(|n| at_ms <= n.at_ms + RETRY_WINDOW_MS);
                if let Some(clip) = near.and_then(|n| n.take(&self.ring)) {
                    self.out.push_back((Label::Pos, clip));
                }
            }
            Event::ExpectWake { at_ms } => self.expect = Some((at_ms, None)),
            Event::FalseWake { at_ms } => {
                if let Some(w) = self.wake.as_mut().filter(|w| at_ms < w.peak.at_ms + FALSE_WAKE_WINDOW_MS) {
                    w.label = Some(Label::Neg);
                    self.resolve();
                }
            }
        }
        self.out.pop_front()
    }

    /// Le réveil en attente rend son extrait, et celui du presque-réveil d'avant sauf si le réveil était faux.
    fn resolve(&mut self) {
        let Some(w) = self.wake.take() else { return };
        match w.label {
            Some(Label::Neg) => self.out.extend(w.neg.map(|c| (Label::Neg, c))),
            Some(Label::Pos) => {
                self.out.extend(w.peak.take(&self.ring).map(|c| (Label::Pos, c)));
                self.out.extend(w.near.map(|c| (Label::Pos, c)));
            }
            None => self.out.extend(w.near.map(|c| (Label::Pos, c))),
        }
    }

    /// WAV 16 kHz mono i16 dans `{pos,neg}/<horodatage>.wav`, puis rotation : 2 000 fichiers et 200 Mo au plus.
    pub fn save(&mut self, label: Label, clip: &[f32]) -> std::io::Result<PathBuf> {
        let sub = self.dir.join(match label { Label::Pos => "pos", Label::Neg => "neg" });
        std::fs::create_dir_all(&sub)?;
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let path = sub.join(format!("{ms:013}-{:08}.wav", SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, wav_16k_mono(clip))?;
        self.rotate(MAX_FILES, MAX_BYTES);
        Ok(path)
    }

    /// Les plus anciens partent d'abord (le nom commence par l'horodatage), `pos` et `neg` ensemble.
    fn rotate(&self, max_files: usize, max_bytes: u64) {
        let mut files: Vec<_> = ["pos", "neg"]
            .iter()
            .filter_map(|d| std::fs::read_dir(self.dir.join(d)).ok())
            .flatten()
            .flatten()
            .map(|e| (e.file_name(), e.metadata().map_or(0, |m| m.len()), e.path()))
            .collect();
        files.sort();
        let mut bytes: u64 = files.iter().map(|f| f.1).sum();
        let mut n = files.len();
        for (_, len, path) in files {
            if n <= max_files && bytes <= max_bytes {
                break;
            }
            let _ = std::fs::remove_file(path);
            n -= 1;
            bytes -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collecteur nourri par pas de 80 ms ; chaque échantillon vaut son indice, ce qui situe un extrait dans le temps.
    struct Sim { c: Collector, out: Vec<(Label, Vec<f32>)> }

    impl Sim {
        fn new() -> Self { Self { c: Collector::new(PathBuf::from("inutilisé")), out: Vec::new() } }
        fn now(&self) -> u64 { self.c.now_ms() }
        fn push(&mut self) {
            let i = self.now() * 16;
            self.c.push_audio(&(i..i + 1280).map(|i| i as f32).collect::<Vec<_>>());
        }
        /// `ms` d'audio sans score (enregistrement de la demande).
        fn feed(&mut self, ms: u64) {
            for _ in 0..ms / 80 {
                self.push();
            }
        }
        /// `ms` d'audio écouté ; `score(t)` est le score du pas qui finit à `t`.
        fn run(&mut self, ms: u64, score: impl Fn(u64) -> f32) {
            for _ in 0..ms / 80 {
                self.push();
                let at_ms = self.now();
                self.on(Event::Score { at_ms, score: score(at_ms) });
            }
        }
        fn on(&mut self, e: Event) {
            if let Some(x) = self.c.on(e) {
                self.out.push(x);
            }
        }
        /// « Dis Iris » détecté sur le pas suivant ; rend son instant.
        fn wake(&mut self) -> u64 {
            self.run(80, |_| 0.999);
            let at_ms = self.now();
            self.on(Event::Woke { at_ms });
            at_ms
        }
        /// Une seule pointe `v` sur le pas suivant ; rend son instant.
        fn peak(&mut self, v: f32) -> u64 {
            self.run(80, |_| v);
            self.now()
        }
    }

    fn quiet(_: u64) -> f32 { 0.001 }

    /// Extrait positif autour du pic `at` : 1,6 s finissant 0,2 s après.
    fn is_pos_around(x: &(Label, Vec<f32>), at: u64) -> bool {
        x.0 == Label::Pos && x.1.len() == 25_600 && x.1[0] == ((at + 200 - 1600) * 16) as f32
    }

    #[test]
    fn positive_clip_is_1600ms_ending_200ms_after_peak() {
        let mut s = Sim::new();
        s.run(3000, quiet);
        let t = s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.run(10_100, quiet);
        assert_eq!(s.out.len(), 1);
        let (label, clip) = &s.out[0];
        assert_eq!(*label, Label::Pos);
        assert_eq!(clip.len(), (POS_BEFORE_MS + POS_AFTER_MS) as usize * 16);
        assert_eq!(clip[0], ((t - POS_BEFORE_MS) * 16) as f32);
        assert_eq!(*clip.last().unwrap(), ((t + POS_AFTER_MS) * 16 - 1) as f32);
    }

    #[test]
    fn expect_then_wake_is_pos_around_wake() {
        let mut s = Sim::new();
        s.run(2000, quiet);
        s.on(Event::ExpectWake { at_ms: s.now() });
        s.run(1000, quiet);
        let t = s.wake();
        s.feed(1000);
        s.on(Event::NoSpeech); // le bouton l'emporte
        s.run(10_100, quiet);
        assert_eq!(s.out.len(), 1);
        assert!(is_pos_around(&s.out[0], t));
    }

    #[test]
    fn expect_without_wake_takes_best_score_above_min() {
        let mut s = Sim::new();
        s.run(2000, quiet);
        s.on(Event::ExpectWake { at_ms: s.now() });
        s.run(1000, |_| 0.1);
        let t = s.peak(0.2);
        s.run(2000, |_| 0.1);
        assert!(s.out.is_empty(), "fenêtre pas finie");
        s.run(2000, quiet);
        assert_eq!(s.out.len(), 1);
        assert!(is_pos_around(&s.out[0], t));
    }

    #[test]
    fn expect_wake_silence_writes_nothing() {
        for v in [0.001, EXPECT_MIN_SCORE - 0.01] {
            let mut s = Sim::new();
            s.run(2000, quiet);
            s.on(Event::ExpectWake { at_ms: s.now() });
            s.run(8000, |_| v);
            assert!(s.out.is_empty(), "{v}");
        }
    }

    #[test]
    fn false_wake_is_neg_and_no_pos() {
        let mut s = Sim::new();
        s.run(4000, quiet);
        let t = s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.run(3000, quiet);
        s.on(Event::FalseWake { at_ms: s.now() });
        assert_eq!(s.out.len(), 1);
        let (label, clip) = &s.out[0];
        assert_eq!(*label, Label::Neg);
        assert_eq!(clip.len(), NEG_MS as usize * 16);
        assert_eq!(*clip.last().unwrap(), (t * 16 - 1) as f32, "finit au réveil");
        s.run(10_000, quiet);
        assert_eq!(s.out.len(), 1, "aucun positif pour ce réveil");
        // Plus de 10 s après : le bouton ne change plus rien.
        let mut s = Sim::new();
        s.run(4000, quiet);
        s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.run(10_100, quiet);
        s.on(Event::FalseWake { at_ms: s.now() });
        assert_eq!(s.out.len(), 1);
        assert_eq!(s.out[0].0, Label::Pos);
    }

    #[test]
    fn wake_then_command_is_pos() {
        let mut s = Sim::new();
        s.run(3000, quiet);
        let t = s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.run(8800, quiet);
        assert!(s.out.is_empty(), "un faux réveil peut encore être signalé");
        s.run(1100, quiet);
        assert_eq!(s.out.len(), 1);
        assert!(is_pos_around(&s.out[0], t));
    }

    #[test]
    fn wake_then_cancel_is_neg() {
        let mut s = Sim::new();
        s.run(4000, quiet);
        let t = s.wake();
        s.feed(500);
        s.on(Event::Cancelled);
        s.feed(1000); // Échap pendant l'enregistrement : la demande peut encore finir
        s.on(Event::Command);
        s.run(10_100, quiet);
        assert_eq!(s.out.len(), 1);
        assert_eq!(s.out[0].0, Label::Neg);
        assert_eq!(*s.out[0].1.last().unwrap(), (t * 16 - 1) as f32);
    }

    #[test]
    fn wake_then_no_speech_keeps_nothing_unless_false_wake_pressed() {
        let mut s = Sim::new();
        s.run(4000, quiet);
        s.wake();
        s.feed(500);
        s.on(Event::NoSpeech);
        s.run(10_100, quiet);
        assert!(s.out.is_empty(), "réveil puis silence : rien");

        let mut s = Sim::new();
        s.run(4000, quiet);
        let t = s.wake();
        s.feed(500);
        s.on(Event::NoSpeech);
        s.on(Event::FalseWake { at_ms: t + 600 });
        assert_eq!(s.out.len(), 1);
        assert_eq!(s.out[0].0, Label::Neg);
    }

    #[test]
    fn near_miss_then_wake_or_manual_start_is_pos() {
        let mut s = Sim::new();
        s.run(3000, quiet);
        let miss = s.peak(0.5);
        s.run(2000, quiet);
        let t = s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.run(10_200, quiet);
        assert_eq!(s.out.len(), 2);
        assert!(s.out.iter().any(|x| is_pos_around(x, miss)), "le presque-réveil");
        assert!(s.out.iter().any(|x| is_pos_around(x, t)), "le réveil");

        let mut s = Sim::new();
        s.run(3000, quiet);
        let miss = s.peak(0.9);
        s.run(3000, quiet);
        s.on(Event::ManualStart { at_ms: s.now() });
        assert_eq!(s.out.len(), 1);
        assert!(is_pos_around(&s.out[0], miss));
    }

    #[test]
    fn near_miss_alone_is_nothing() {
        let mut s = Sim::new();
        s.run(3000, quiet);
        s.peak(0.5);
        s.run(5500, quiet);
        s.on(Event::ManualStart { at_ms: s.now() });
        assert!(s.out.is_empty());
        let t = s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.run(10_100, quiet);
        assert_eq!(s.out.len(), 1, "le réveil seulement");
        assert!(is_pos_around(&s.out[0], t));
    }

    #[test]
    fn short_ring_writes_at_least_one_second_or_nothing() {
        let mut s = Sim::new();
        s.run(1200, quiet);
        s.wake();
        s.feed(1000);
        s.on(Event::FalseWake { at_ms: s.now() });
        assert_eq!(s.out[0].1.len(), 1280 * 16, "ce qui existe, sans remplissage");
        let mut s = Sim::new();
        s.run(800, quiet);
        s.wake();
        s.on(Event::FalseWake { at_ms: s.now() });
        assert!(s.out.is_empty(), "moins d'une seconde");
    }

    #[test]
    fn no_clip_spans_a_mic_closure() {
        // Audio A (dont la fin d'une demande), micro fermé, puis rouvert : rien de A après coup.
        let mut s = Sim::new();
        s.run(5000, quiet);
        s.c.gap();
        let reopened = (s.now() * 16) as f32;
        s.run(1500, quiet);
        s.wake();
        s.feed(500);
        s.on(Event::FalseWake { at_ms: s.now() });
        assert_eq!(s.out.len(), 1);
        assert!(s.out[0].1.iter().all(|v| *v >= reopened), "négatif sans A");
        // Presque-réveil de A non encore copié, push-to-talk après la réouverture : rien.
        let mut s = Sim::new();
        s.run(3000, quiet);
        s.run(80, |_| 0.5);
        s.c.gap();
        s.run(1500, quiet);
        s.on(Event::ManualStart { at_ms: s.now() });
        assert!(s.out.is_empty());
        // Le négatif du réveil, pris au réveil, survit à la fermeture due au tour d'IRIS.
        let mut s = Sim::new();
        s.run(4000, quiet);
        let t = s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.c.gap();
        s.run(2000, quiet);
        s.on(Event::FalseWake { at_ms: s.now() });
        assert_eq!(s.out.len(), 1);
        assert_eq!(s.out[0].0, Label::Neg);
        assert_eq!(*s.out[0].1.last().unwrap(), (t * 16 - 1) as f32);
    }

    #[test]
    fn drop_pending_forgets_wake_near_and_expect() {
        let mut s = Sim::new();
        s.run(3000, quiet);
        s.peak(0.5);
        s.on(Event::ExpectWake { at_ms: s.now() });
        s.run(2000, quiet);
        s.wake();
        s.feed(1000);
        s.on(Event::Command);
        s.c.drop_pending();
        s.run(12_000, quiet);
        s.on(Event::FalseWake { at_ms: s.now() });
        assert!(s.out.is_empty(), "rien n'est écrit pour ce qui attendait");
    }

    #[test]
    fn rotation_keeps_newest_2000() {
        let dir = tempfile::tempdir().unwrap();
        let pos = dir.path().join("pos");
        std::fs::create_dir_all(&pos).unwrap();
        // 2 003 fichiers plus anciens (nom triant avant tout horodatage réel), puis 2 sauvegardes.
        let old: Vec<PathBuf> = (0..2003).map(|i| pos.join(format!("0000000000000-{i:08}.wav"))).collect();
        for p in &old {
            std::fs::write(p, b"x").unwrap();
        }
        let mut c = Collector::new(dir.path().to_path_buf());
        let saved = [c.save(Label::Neg, &[0.1; 160]).unwrap(), c.save(Label::Pos, &[0.1; 160]).unwrap()];
        let count = |d: &str| std::fs::read_dir(dir.path().join(d)).unwrap().count();
        assert_eq!(count("pos") + count("neg"), MAX_FILES);
        assert!(old[..5].iter().all(|p| !p.exists()), "les 5 plus anciens");
        assert!(old[5..].iter().chain(&saved).all(|p| p.exists()));
        assert_eq!(std::fs::read(&saved[1]).unwrap(), oniris_core::voice::audio::wav_16k_mono(&[0.1; 160]));
    }

    #[test]
    fn rotation_caps_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let neg = dir.path().join("neg");
        std::fs::create_dir_all(&neg).unwrap();
        for i in 0..3 {
            std::fs::write(neg.join(format!("{i}.wav")), [0u8; 100]).unwrap();
        }
        Collector::new(dir.path().to_path_buf()).rotate(MAX_FILES, 250);
        assert!(!neg.join("0.wav").exists() && neg.join("1.wav").exists() && neg.join("2.wav").exists());
    }

    #[test]
    fn count_and_clear_only_touch_learned_wavs() {
        let dir = tempfile::tempdir().unwrap();
        for (d, n) in [("pos", 3), ("neg", 2)] {
            std::fs::create_dir_all(dir.path().join(d)).unwrap();
            for i in 0..n {
                std::fs::write(dir.path().join(d).join(format!("{i}.wav")), b"x").unwrap();
            }
        }
        std::fs::write(dir.path().join("pos").join("note.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("keep.wav"), b"x").unwrap();
        assert_eq!(count_in(dir.path()), (3, 2));
        assert_eq!(clear_in(dir.path()), 5);
        assert_eq!(count_in(dir.path()), (0, 0));
        assert!(dir.path().join("pos").join("note.txt").exists() && dir.path().join("keep.wav").exists());
        assert_eq!(count_in(&dir.path().join("absent")), (0, 0));
    }
}
