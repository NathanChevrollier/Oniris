//! Audio pur : mixage mono, rééchantillonnage 16 kHz, niveau, WAV et durée de parole.

/// Mixe en mono (moyenne des canaux) puis rééchantillonne à 16 kHz par interpolation linéaire.
/// Longueur de sortie : `frames * 16000 / rate` (arrondi vers le bas).
pub fn resample_to_16k(input: &[f32], rate: u32, channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let mono: Vec<f32> = input.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    if rate == 0 || mono.is_empty() { return Vec::new(); }
    if rate == 16_000 { return mono; }
    let out_len = (mono.len() as u64 * 16_000 / rate as u64) as usize;
    let step = rate as f64 / 16_000.0;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let k = pos as usize;
            let frac = (pos - k as f64) as f32;
            let a = mono[k];
            let b = *mono.get(k + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

/// Version en flux de `resample_to_16k` : même interpolation linéaire, état gardé entre les appels.
pub struct Resampler16k {
    step: f64,
    channels: usize,
    passthrough: bool,
    prev: Option<f32>,
    pos: f64, // position de la prochaine sortie, relative à `prev`
}

impl Resampler16k {
    pub fn new(rate: u32, channels: u16) -> Self {
        Self { step: rate as f64 / 16_000.0, channels: channels.max(1) as usize, passthrough: rate == 16_000, prev: None, pos: 0.0 }
    }

    /// Ajoute des échantillons entrelacés ; `out` reçoit le mono 16 kHz produit (sans être effacé).
    pub fn push(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        if self.step <= 0.0 { return; }
        let ch = self.channels;
        for f in interleaved.chunks_exact(ch) {
            let s = f.iter().sum::<f32>() / ch as f32;
            if self.passthrough { out.push(s); continue; }
            let Some(p) = self.prev.replace(s) else { continue };
            while self.pos < 1.0 {
                out.push(p + (s - p) * self.pos as f32);
                self.pos += self.step;
            }
            self.pos -= 1.0;
        }
    }
}

/// Valeur efficace d'une trame (0 si vide).
pub fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() { return 0.0 }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

/// WAV PCM 16 bits, 16 kHz mono, en-tête canonique de 44 octets.
pub fn wav_16k_mono(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&16_000u32.to_le_bytes());
    w.extend_from_slice(&32_000u32.to_le_bytes()); // octets/s
    w.extend_from_slice(&2u16.to_le_bytes()); // alignement
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples { w.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()); }
    w
}

/// Durée cumulée (ms) des trames de 20 ms dont le niveau dépasse `threshold`.
pub fn speech_ms(samples: &[f32], threshold: f32) -> u32 {
    samples.as_chunks::<320>().0.iter().filter(|f| rms(*f) > threshold).count() as u32 * 20
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32, channels: usize) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    #[test]
    fn resample_48k_stereo_garde_la_longueur_et_la_frequence() {
        let out = resample_to_16k(&sine(440.0, 48_000, 1.0, 2), 48_000, 2);
        assert!((out.len() as i64 - 16_000).abs() <= 1);
        let crossings = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((crossings as i64 - 440).abs() <= 2, "{crossings}");
    }

    #[test]
    fn streaming_equals_one_shot() {
        let input = sine(440.0, 48_000, 2.0, 2);
        let expected = resample_to_16k(&input, 48_000, 2);
        let mut r = Resampler16k::new(48_000, 2);
        let mut got = Vec::new();
        let (mut i, mut k) = (0, 0);
        while i < input.len() {
            let n = ([1usize, 7, 480, 1023, 4800][k % 5] * 2).min(input.len() - i);
            r.push(&input[i..i + n], &mut got);
            i += n;
            k += 1;
        }
        assert!((got.len() as i64 - expected.len() as i64).abs() <= 1, "{} vs {}", got.len(), expected.len());
        let max = got.iter().zip(&expected).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(max <= 1e-4, "{max}");
    }

    #[test]
    fn sixteen_k_mono_passthrough() {
        let input = sine(440.0, 16_000, 0.1, 1);
        let mut out = Vec::new();
        Resampler16k::new(16_000, 1).push(&input, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn resample_cas_limites() {
        assert!(resample_to_16k(&[], 48_000, 2).is_empty());
        assert_eq!(resample_to_16k(&[0.5, 0.5], 16_000, 2), vec![0.5]);
    }

    #[test]
    fn wav_a_un_en_tete_valide() {
        let w = wav_16k_mono(&[0.0, 1.0, -1.0]);
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u16::from_le_bytes(w[34..36].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 6);
        assert_eq!(i16::from_le_bytes(w[46..48].try_into().unwrap()), 32767);
    }

    #[test]
    fn speech_ms_silence_et_signal() {
        assert_eq!(speech_ms(&vec![0.0; 16_000], 0.01), 0);
        let half = sine(440.0, 16_000, 0.5, 1);
        assert_eq!(speech_ms(&half, 0.01), 500);
        assert!(rms(&half) > 0.3);
    }
}
