//! Mot d'activation « Iris » : détecteur openWakeWord en flux, en Rust pur (tract).
//! Reproduit `AudioFeatures` (openwakeword/utils.py) : mel sur 1 280 + 480 échantillons, embedding sur 76 trames,
//! classifieur sur 16 embeddings. L'audio ne vit qu'ici, en mémoire (au plus 1 760 échantillons).
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tract_onnx::prelude::*;

/// Un score par pas de 80 ms à 16 kHz.
pub const WAKE_CHUNK: usize = 1280;
const MEL_INPUT: usize = WAKE_CHUNK + 480;
const MEL_BINS: usize = 32;
const MEL_WINDOW: usize = 76;
const MEL_MAX: usize = 10 * 97;
const EMB_DIM: usize = 96;
const EMB_MAX: usize = 120;
const EMB_WINDOW: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum WakeError {
    #[error("fichier du mot d'activation introuvable : {0}")]
    Missing(String),
    #[error("fichier du mot d'activation corrompu : {0}")]
    Corrupt(String),
    #[error("modèle du mot d'activation illisible : {0}")]
    Model(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeModels {
    pub mel: PathBuf,
    pub embedding: PathBuf,
    pub classifier: PathBuf,
}

impl WakeModels {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            mel: dir.join("melspectrogram.onnx"),
            embedding: dir.join("embedding_model.onnx"),
            classifier: dir.join("dis_iris.onnx"),
        }
    }
}

fn name(p: &Path) -> String { p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()) }

/// Compare les trois modèles de `dir` à `dir/SHA256SUMS` (format `sha256sum`, fins de ligne LF ou CRLF).
pub fn verify(dir: &Path) -> Result<(), WakeError> {
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).map_err(|_| WakeError::Missing("SHA256SUMS".into()))?;
    let m = WakeModels::in_dir(dir);
    for p in [&m.mel, &m.embedding, &m.classifier] {
        let n = name(p);
        let bytes = std::fs::read(p).map_err(|_| WakeError::Missing(n.clone()))?;
        let hex: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        let listed = sums.lines().filter_map(|l| l.trim().split_once(char::is_whitespace))
            .any(|(h, f)| h.eq_ignore_ascii_case(&hex) && f.trim().trim_start_matches('*') == n);
        if !listed { return Err(WakeError::Corrupt(n)); }
    }
    Ok(())
}

type Plan = Arc<TypedRunnableModel>;

fn load_plan(p: &Path, shape: &[usize]) -> Result<Plan, WakeError> {
    if !p.is_file() { return Err(WakeError::Missing(name(p))); }
    let model = || -> TractResult<Plan> {
        tract_onnx::onnx().model_for_path(p)?.with_input_fact(0, f32::fact(shape).into())?.into_optimized()?.into_runnable()
    };
    model().map_err(|e| WakeError::Model(format!("{} : {e:#}", name(p))))
}

fn run(plan: &Plan, shape: &[usize], data: &[f32]) -> Vec<f32> {
    let out = Tensor::from_shape(shape, data).and_then(|t| plan.run(tvec!(t.into_tvalue())));
    // Les formes sont fixées au chargement : une erreur ici serait un bug, pas une entrée invalide.
    let out = out.expect("inférence du mot d'activation");
    out[0].to_plain_array_view::<f32>().expect("sortie f32").iter().copied().collect()
}

pub struct WakeDetector {
    mel_model: Plan,
    emb_model: Plan,
    cls_model: Plan,
    /// Derniers 1 760 échantillons, à l'échelle i16 (attendue par le modèle mel).
    raw: Vec<f32>,
    pending: Vec<f32>,
    /// Trames mel à plat (× 32).
    mel: Vec<f32>,
    /// Embeddings à plat (× 96).
    emb: Vec<f32>,
}

impl WakeDetector {
    pub fn load(m: &WakeModels) -> Result<Self, WakeError> {
        let mut d = Self {
            mel_model: load_plan(&m.mel, &[1, MEL_INPUT])?,
            emb_model: load_plan(&m.embedding, &[1, MEL_WINDOW, MEL_BINS, 1])?,
            cls_model: load_plan(&m.classifier, &[1, EMB_WINDOW, EMB_DIM])?,
            raw: Vec::new(),
            pending: Vec::with_capacity(WAKE_CHUNK),
            mel: Vec::new(),
            emb: Vec::new(),
        };
        d.reset();
        Ok(d)
    }

    /// Vide les tampons. Comme openWakeWord, le mel repart de 76 trames à 1 ; contrairement à lui, pas d'embeddings
    /// aléatoires : le score vaut 0 tant que 16 embeddings réels manquent.
    pub fn reset(&mut self) {
        // Écart voulu : 480 zéros devant le premier pas, pour garder une seule forme d'entrée mel (sans effet après ~25 pas).
        self.raw = vec![0.0; MEL_INPUT - WAKE_CHUNK];
        self.pending.clear();
        self.mel = vec![1.0; MEL_WINDOW * MEL_BINS];
        self.emb.clear();
    }

    /// Accepte n'importe quelle longueur ; rend un score par pas complet de `WAKE_CHUNK` échantillons.
    pub fn push(&mut self, samples_16k: &[f32]) -> Vec<f32> {
        let mut scores = Vec::new();
        for &s in samples_16k {
            self.pending.push((s * 32768.0).clamp(-32768.0, 32767.0));
            if self.pending.len() == WAKE_CHUNK {
                scores.push(self.step());
            }
        }
        scores
    }

    fn step(&mut self) -> f32 {
        self.raw.extend(self.pending.drain(..));
        self.raw.drain(..self.raw.len() - MEL_INPUT);
        let spec = run(&self.mel_model, &[1, MEL_INPUT], &self.raw);
        self.mel.extend(spec.iter().map(|v| v / 10.0 + 2.0));
        trim(&mut self.mel, MEL_MAX * MEL_BINS);
        let window = &self.mel[self.mel.len() - MEL_WINDOW * MEL_BINS..];
        let e = run(&self.emb_model, &[1, MEL_WINDOW, MEL_BINS, 1], window);
        self.emb.extend(e);
        trim(&mut self.emb, EMB_MAX * EMB_DIM);
        if self.emb.len() < EMB_WINDOW * EMB_DIM { return 0.0; }
        run(&self.cls_model, &[1, EMB_WINDOW, EMB_DIM], &self.emb[self.emb.len() - EMB_WINDOW * EMB_DIM..])[0]
    }
}

fn trim(v: &mut Vec<f32>, max: usize) {
    if v.len() > max { v.drain(..v.len() - max); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri") }
    fn res() -> PathBuf { root().join("resources/wake") }
    fn detector() -> WakeDetector { WakeDetector::load(&WakeModels::in_dir(&res())).unwrap() }
    fn argmax(v: &[f32]) -> usize { (0..v.len()).max_by(|&a, &b| v[a].total_cmp(&v[b])).unwrap() }

    #[test]
    fn fixture_scores_match_python() {
        let dir = root().join("tests/fixtures/wake/dis_iris");
        let fx: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("scores.json")).unwrap()).unwrap();
        let expected: Vec<f32> = fx["scores"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
        let from = fx["compare_from"].as_u64().unwrap() as usize;
        let wav = std::fs::read(dir.join("sequence.wav")).unwrap();
        assert_eq!(&wav[36..40], b"data", "en-tête WAV canonique attendu");
        let samples: Vec<f32> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();

        let mut d = detector();
        let rust: Vec<f32> = samples.chunks(1000).flat_map(|c| d.push(c)).collect();
        assert_eq!(rust.len(), expected.len());
        for i in from..expected.len() {
            assert!((rust[i] - expected[i]).abs() <= 0.02, "pas {i} : rust {} python {}", rust[i], expected[i]);
        }
        let worst = (from..expected.len()).map(|i| (rust[i] - expected[i]).abs()).fold(0.0, f32::max);
        println!("écart max {worst:.5}, pic rust {} ({:.4}) python {} ({:.4})", argmax(&rust), rust[argmax(&rust)], argmax(&expected), expected[argmax(&expected)]);
        assert_eq!(argmax(&rust), argmax(&expected));
    }

    #[test]
    fn silence_never_triggers() {
        let mut d = detector();
        let s = d.push(&vec![0.0; 160_000]);
        assert_eq!(s.len(), 125);
        assert!(s.iter().all(|&x| x < 0.1), "{s:?}");
    }

    #[test]
    fn verify_detects_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        for f in ["SHA256SUMS", "melspectrogram.onnx", "embedding_model.onnx", "dis_iris.onnx"] {
            std::fs::copy(res().join(f), tmp.path().join(f)).unwrap();
        }
        verify(tmp.path()).unwrap();
        let p = tmp.path().join("dis_iris.onnx");
        let mut b = std::fs::read(&p).unwrap();
        b[100] ^= 1;
        std::fs::write(&p, b).unwrap();
        assert!(matches!(verify(tmp.path()), Err(WakeError::Corrupt(_))));
    }

    #[test]
    fn missing_model_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(verify(tmp.path()), Err(WakeError::Missing(_))));
        assert!(matches!(WakeDetector::load(&WakeModels::in_dir(tmp.path())), Err(WakeError::Missing(_))));
    }

    #[test]
    #[ignore]
    fn sixty_seconds_under_three() {
        let mut d = detector();
        let mut x: u32 = 1;
        let noise: Vec<f32> = (0..16_000 * 60)
            .map(|_| { x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (x >> 8) as f32 / (1u32 << 24) as f32 * 0.02 - 0.01 })
            .collect();
        let t = std::time::Instant::now();
        let n: usize = noise.chunks(WAKE_CHUNK).map(|c| d.push(c).len()).sum();
        let dt = t.elapsed();
        println!("60 s d'audio traitées en {dt:?} ({n} scores)");
        assert!(dt.as_secs_f32() < 3.0, "{dt:?}");
    }
}
