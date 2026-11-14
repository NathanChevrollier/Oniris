//! Verre automatique : ce qu'il y a derrière le panneau règle son opacité. L'image réduite reste en mémoire
//! le temps du calcul : rien n'est enregistré ni envoyé, seule la valeur 0..1 part au panneau.

use super::geometry::Rect;
use tauri::{AppHandle, Emitter};

const W: i32 = 32;
const H: i32 = 18;

/// Intensité 0..1 depuis des pixels BGRA : 0 = fond sombre et uni (verre léger), 1 = fond clair ou chargé.
pub fn intensity(bgra: &[u8]) -> f32 {
    let lum: Vec<f32> = bgra
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| (0.0722 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.2126 * p[2] as f32) / 255.0)
        .collect();
    if lum.is_empty() {
        return 0.5;
    }
    let n = lum.len() as f32;
    let mean = lum.iter().sum::<f32>() / n;
    let var = lum.iter().map(|l| (l - mean) * (l - mean)).sum::<f32>() / n;
    // Écart type de 0,25 = fond très chargé (texte, photo contrastée).
    (0.8 * mean + 0.5 * (var.sqrt() / 0.25)).clamp(0.0, 1.0)
}

/// Capture GDI réduite (points échantillonnés, sans lissage : quelques millisecondes) de `r`, en BGRA.
fn capture(r: Rect) -> Option<Vec<u8>> {
    use ::windows::Win32::Graphics::Gdi::*;
    let mut buf = vec![0u8; (W * H * 4) as usize];
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, W, H);
        let old = SelectObject(mem, bmp.into());
        SetStretchBltMode(mem, COLORONCOLOR);
        let ok = StretchBlt(mem, 0, 0, W, H, Some(screen), r.x, r.y, r.w as i32, r.h as i32, SRCCOPY).as_bool();
        SelectObject(mem, old);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: W,
                biHeight: -H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = if ok { GetDIBits(mem, bmp, 0, H as u32, Some(buf.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS) } else { 0 };
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        (lines == H).then_some(buf)
    }
}

/// Mesure, sur un fil, la zone d'écran sous la surface du panneau (haut et centre de `m`) et émet `oniris://glass`.
/// Appelé avant `show` : la fenêtre est encore cachée, ou transparente à sa première image.
pub fn sample(app: &AppHandle, m: Rect) {
    let r = Rect { x: m.x + (m.w / 7) as i32, y: m.y, w: m.w * 5 / 7, h: m.h * 4 / 5 };
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(px) = capture(r) {
            let _ = app.emit("oniris://glass", intensity(&px));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::intensity;

    fn uni(v: u8) -> Vec<u8> { [v, v, v, 255].repeat(32 * 18) }

    #[test]
    fn fond_sombre_uni_leger_clair_ou_charge_opaque() {
        assert!(intensity(&uni(10)) < 0.1);
        assert!(intensity(&uni(240)) > 0.7);
        // damier noir/blanc : moyenne moyenne, variance maximale
        let damier: Vec<u8> = (0..32 * 18).flat_map(|i| if i % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        assert_eq!(intensity(&damier), 1.0);
        // fond sombre mais chargé : plus opaque que le même fond uni
        let sombre_charge: Vec<u8> = (0..32 * 18).flat_map(|i| if i % 3 == 0 { [150, 150, 150, 255] } else { [20, 20, 20, 255] }).collect();
        assert!(intensity(&sombre_charge) > intensity(&uni(63)) + 0.2);
        assert_eq!(intensity(&[]), 0.5);
    }
}
