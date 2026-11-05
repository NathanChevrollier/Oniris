//! Musique : titre en cours (lu par Windows, SMTC, côté src-tauri), pochette et couleurs dominantes.
pub mod tools;

use image::RgbaImage;
use serde::Serialize;

/// Lecteur d'où vient le titre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source { Spotify, Other }

/// Titre en cours. Contenu utilisateur : jamais journalisé.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `data:image/png;base64,…`, 300 px au plus.
    pub cover: Option<String>,
    /// 3 couleurs dominantes de la pochette, `#rrggbb`.
    pub colors: Vec<String>,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Instant de `position_ms` (ms Unix) : l'interface extrapole la position pendant la lecture.
    pub updated_ms: i64,
    pub source: Source,
}

/// Commandes de lecture (SMTC côté src-tauri, faux lecteur dans les tests). Appels bloquants ;
/// l'erreur est un message en français, prêt à afficher.
pub trait MediaControl: Send + Sync {
    fn toggle(&self) -> Result<(), String>;
    fn next(&self) -> Result<(), String>;
    fn previous(&self) -> Result<(), String>;
    fn seek(&self, position_ms: i64) -> Result<(), String>;
}

pub fn is_spotify(app_id: &str) -> bool { app_id.to_lowercase().contains("spotify") }

/// Session à suivre : la première dont l'identifiant d'application contient « Spotify » ;
/// `None` : prendre la session courante de Windows.
pub fn pick_session<'a>(app_ids: impl IntoIterator<Item = &'a str>) -> Option<usize> {
    app_ids.into_iter().position(is_spotify)
}

/// Au plus une émission par `gap_ms` (250 ms : 4 par seconde).
pub struct Throttle { gap_ms: i64, last: Option<i64> }

impl Throttle {
    pub const fn new(gap_ms: i64) -> Self { Self { gap_ms, last: None } }
    /// Attente avant la prochaine émission permise.
    pub fn wait_ms(&self, now: i64) -> i64 { self.last.map_or(0, |l| (l + self.gap_ms - now).max(0)) }
    pub fn mark(&mut self, now: i64) { self.last = Some(now); }
}

/// Couleurs dominantes : histogramme quantifié sur 4 bits par canal (pixels trop sombres, trop clairs
/// ou transparents ignorés), trié par saturation × poids, sans deux couleurs trop proches. Déterministe.
pub fn dominant_colors(img: &RgbaImage, k: usize) -> Vec<[u8; 3]> {
    let mut bins = vec![(0u32, [0u64; 3]); 4096];
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
        if a < 128 || hi < 40 || lo > 215 {
            continue;
        }
        let bin = &mut bins[(r as usize >> 4) << 8 | (g as usize >> 4) << 4 | b as usize >> 4];
        bin.0 += 1;
        for (s, c) in bin.1.iter_mut().zip([r, g, b]) {
            *s += c as u64;
        }
    }
    let mut cands: Vec<([u8; 3], f64)> = bins.iter().filter(|b| b.0 > 0).map(|(n, sum)| {
        let c = sum.map(|s| (s / *n as u64) as u8);
        let (hi, lo) = (c.iter().max().copied().unwrap_or(0) as f64, c.iter().min().copied().unwrap_or(0) as f64);
        let sat = if hi == 0.0 { 0.0 } else { (hi - lo) / hi };
        (c, (sat + 0.05) * *n as f64) // + 0.05 : une pochette en gris garde ses couleurs
    }).collect();
    cands.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut out: Vec<[u8; 3]> = Vec::with_capacity(k);
    for (c, _) in cands {
        if out.len() == k {
            break;
        }
        let far = |o: &[u8; 3]| o.iter().zip(c).map(|(&a, b)| (a as i32 - b as i32).pow(2)).sum::<i32>() > 48 * 48;
        if out.iter().all(far) {
            out.push(c);
        }
    }
    out
}

pub fn hex([r, g, b]: [u8; 3]) -> String { format!("#{r:02x}{g:02x}{b:02x}") }

/// Pochette brute (png, jpeg…) → URL `data:image/png;base64,…` réduite à 300 px et ses 3 couleurs dominantes.
pub fn cover(bytes: &[u8]) -> Option<(String, Vec<String>)> {
    use base64::Engine;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let img = if img.width() > 300 || img.height() > 300 { img.thumbnail(300, 300) } else { img };
    let colors = dominant_colors(&img.to_rgba8(), 3).into_iter().map(hex).collect();
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some((format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)), colors))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// Moitié gauche rouge, moitié droite bleue, bande blanche en haut (plus grande que chacune).
    fn half_red_blue() -> RgbaImage {
        RgbaImage::from_fn(40, 40, |x, y| match (x, y) {
            (_, 0..=19) => Rgba([255, 255, 255, 255]),
            (0..=19, _) => Rgba([220, 20, 30, 255]),
            _ => Rgba([20, 40, 210, 255]),
        })
    }

    #[test]
    fn rouge_et_bleu_retrouves_blanc_ignore() {
        let c = dominant_colors(&half_red_blue(), 3);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c.contains(&[220, 20, 30]) && c.contains(&[20, 40, 210]), "{c:?}");
        assert_eq!(dominant_colors(&half_red_blue(), 3), c, "déterministe");
    }

    #[test]
    fn couleurs_triees_par_saturation_fois_poids_et_distinctes() {
        // 3/4 rouge vif, 1/4 rouge presque identique (fusionné), quelques pixels gris.
        let img = RgbaImage::from_fn(20, 20, |x, y| match (x, y) {
            (0, 0) => Rgba([120, 120, 120, 255]),
            (0..=14, _) => Rgba([230, 10, 10, 255]),
            _ => Rgba([200, 20, 20, 255]),
        });
        assert_eq!(dominant_colors(&img, 3), vec![[230, 10, 10], [120, 120, 120]]);
        assert_eq!(hex([230, 10, 10]), "#e60a0a");
    }

    #[test]
    fn pochette_reduite_a_300_px_en_png() {
        let big = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(600, 400, image::Rgb([20, 40, 210])));
        let mut bytes = Vec::new();
        big.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg).unwrap();
        let (url, colors) = cover(&bytes).unwrap();
        let b64 = url.strip_prefix("data:image/png;base64,").unwrap();
        let out = image::load_from_memory(&base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (300, 200));
        assert_eq!(colors.len(), 1, "{colors:?}");
        assert!(cover(b"pas une image").is_none());
    }

    #[test]
    fn spotify_prefere_sinon_session_courante() {
        assert_eq!(pick_session(["Chrome", "Spotify.exe"]), Some(1));
        assert_eq!(pick_session(["SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"]), Some(0));
        assert_eq!(pick_session(["Chrome", "Microsoft.ZuneMusic"]), None);
        assert!(is_spotify("spotify.exe") && !is_spotify("chrome.exe"));
    }

    #[test]
    fn au_plus_quatre_emissions_par_seconde() {
        let mut t = Throttle::new(250);
        let mut sent = 0;
        // Un événement toutes les 10 ms pendant une seconde.
        for now in (0..1000).step_by(10) {
            if t.wait_ms(now) == 0 {
                t.mark(now);
                sent += 1;
            }
        }
        assert_eq!(sent, 4);
        assert_eq!(t.wait_ms(800), 200, "dernière émission à 750 ms");
        t.mark(1000);
        assert_eq!(t.wait_ms(1100), 150);
    }
}
