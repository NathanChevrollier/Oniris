//! Verre automatique : ce qu'il y a derrière le panneau règle son opacité. Panneau ouvert, un fil remesure toutes
//! les ~1,5 s (et dès que les fenêtres derrière changent) une image de 64 × 36 points gardée en mémoire le temps
//! du calcul, fenêtres d'Oniris exclues de la capture ; il s'arrête à la fermeture. De la liste des fenêtres, seuls
//! les rectangles, classes et états sont lus (jamais les titres). Rien n'est enregistré ni envoyé.

use super::geometry::Rect;
use crate::state::AppState;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const W: usize = 64;
const H: usize = 36;
/// Contraste visé pour le texte secondaire d'Oniris (`--txt2`, blanc à 62 %) sur le fond résultant : WCAG AA.
const TARGET: f32 = 4.5;
/// Couleur du verre (`--glass`, rgb(22,22,28)) en luminance perçue.
const GLASS: f32 = 0.088;
const FLOOR: f32 = 0.30;
const CEIL: f32 = 0.94;
const PERIOD: Duration = Duration::from_millis(1500);
const TICK: Duration = Duration::from_millis(400);
/// Temps pour que DWM compose une image sans nos fenêtres après `WDA_EXCLUDEFROMCAPTURE`.
const SETTLE: Duration = Duration::from_millis(120);

static LOOP: AtomicU64 = AtomicU64::new(0);

/// Ce que l'on retient d'une capture, tout en 0..1 (luminances perçues, encodées sRGB).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// Clarté que le flou laisse passer (90e centile de la grille lissée 3 × 3) : c'est elle qui limite le contraste.
    pub high: f32,
    /// Densité de détails : écart moyen entre points voisins (texte, photo chargée).
    pub detail: f32,
    pub sat: f32,
    /// Part de zones très claires.
    pub bright: f32,
}

/// Envoyé au panneau (`oniris://glass`) : opacité de base, part de la zone couverte par une application, et pourquoi.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Glass {
    pub base: f32,
    pub app: f32,
    pub tone: &'static str,
}

fn lin(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn enc(y: f32) -> f32 {
    if y <= 0.003_130_8 { 12.92 * y } else { 1.055 * y.powf(1.0 / 2.4) - 0.055 }
}

/// Statistiques d'une grille BGRA de `w` points de large.
pub fn stats(bgra: &[u8], w: usize) -> Option<Stats> {
    let px = bgra.as_chunks::<4>().0;
    if w == 0 || px.len() < w || px.len() % w != 0 {
        return None;
    }
    let h = px.len() / w;
    let mut l = Vec::with_capacity(px.len());
    let (mut sat, mut bright) = (0.0, 0.0);
    for p in px {
        let (b, g, r) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
        let v = enc(0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b));
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        sat += if mx > 0.05 { (mx - mn) / mx } else { 0.0 };
        bright += (v > 0.75) as u8 as f32;
        l.push(v);
    }
    let n = l.len() as f32;
    let at = |x: usize, y: usize| l[y * w + x];
    let mut detail = 0.0;
    let mut blur = Vec::with_capacity(l.len());
    for y in 0..h {
        for x in 0..w {
            detail += (at((x + 1).min(w - 1), y) - at(x, y)).abs() + (at(x, (y + 1).min(h - 1)) - at(x, y)).abs();
            let (mut s, mut k) = (0.0, 0.0);
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    s += at(xx, yy);
                    k += 1.0;
                }
            }
            blur.push(s / k);
        }
    }
    blur.sort_by(f32::total_cmp);
    Some(Stats { high: blur[(blur.len() - 1) * 9 / 10], detail: detail / (2.0 * n), sat: sat / n, bright: bright / n })
}

/// Contraste du texte secondaire (blanc à 62 %) sur un fond de luminance perçue `bg`.
pub fn contrast(bg: f32) -> f32 {
    let t = 0.62 * 0.961 + 0.38 * bg;
    (lin(t) + 0.05) / (lin(bg) + 0.05)
}

/// Fond vu à travers un verre d'opacité `a` posé sur `bg` (le navigateur compose en sRGB).
pub fn through(a: f32, bg: f32) -> f32 {
    a * GLASS + (1.0 - a) * bg
}

/// Plus petite opacité qui garde le texte lisible sur `bg`.
fn needed(bg: f32) -> f32 {
    if contrast(bg) >= TARGET {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let m = (lo + hi) / 2.0;
        if contrast(through(m, bg)) >= TARGET { hi = m } else { lo = m }
    }
    hi
}

/// Opacité de base : lisibilité d'abord, puis plus de verre sur un fond chargé ou très coloré qui distrait.
pub fn base(s: Stats) -> (f32, &'static str) {
    let need = needed(s.high);
    let busy = 0.24 * (s.detail / 0.12).min(1.0) + 0.06 * s.sat + 0.05 * s.bright;
    let a = (need.max(FLOOR) + busy).clamp(FLOOR, CEIL);
    let tone = if need > 0.4 { "clair" } else if busy > 0.1 { "chargé" } else { "sombre" };
    (a, tone)
}

/// Part (0..1) de `r` couverte par `wins`, sur une grille de 16 × 9 points.
pub fn cover(r: Rect, wins: &[Rect]) -> f32 {
    let mut n = 0;
    for j in 0..9 {
        for i in 0..16 {
            let p = (r.x + ((2 * i + 1) * r.w as i32) / 32, r.y + ((2 * j + 1) * r.h as i32) / 18);
            n += wins.iter().any(|w| super::geometry::contains(*w, p)) as u32;
        }
    }
    n as f32 / 144.0
}

/// Nouvelle valeur à envoyer, ou `None` si l'écart reste dans l'hystérésis (pas d'oscillation).
pub fn next(prev: Option<Glass>, s: Stats, cov: f32) -> Option<Glass> {
    let (b, tone) = base(s);
    let was_app = prev.is_some_and(|p| p.app > 0.0);
    let app = if cov >= 0.4 || (was_app && cov >= 0.2) { (cov * 10.0).round() / 10.0 } else { 0.0 };
    let g = Glass { base: b, app, tone };
    match prev {
        Some(p) if (p.base - b).abs() < 0.05 && p.app == app => None,
        _ => Some(g),
    }
}

/// Capture GDI réduite (points échantillonnés, sans lissage : quelques millisecondes) de `r`, en BGRA.
fn capture(r: Rect) -> Option<Vec<u8>> {
    use ::windows::Win32::Graphics::Gdi::*;
    let (w, h) = (W as i32, H as i32);
    let mut buf = vec![0u8; W * H * 4];
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        SetStretchBltMode(mem, COLORONCOLOR);
        let ok = StretchBlt(mem, 0, 0, w, h, Some(screen), r.x, r.y, r.w as i32, r.h as i32, SRCCOPY).as_bool();
        SelectObject(mem, old);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = if ok { GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS) } else { 0 };
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        (lines == h).then_some(buf)
    }
}

/// Fenêtres d'application visibles d'autres processus (poignée, rectangle), de haut en bas. Exclus : bureau
/// et barre des tâches, fenêtres masquées par DWM, réduites, d'outils ou traversées par la souris. Titres non lus.
fn app_windows() -> Vec<(isize, Rect)> {
    use ::windows::core::BOOL;
    use ::windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use ::windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    use ::windows::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let out = unsafe { &mut *(l.0 as *mut Vec<(isize, Rect)>) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
        if pid == std::process::id()
            || !unsafe { IsWindowVisible(h) }.as_bool()
            || unsafe { IsIconic(h) }.as_bool()
            || ex & (WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0) != 0
        {
            return BOOL(1);
        }
        let mut cloaked = 0u32;
        let size = std::mem::size_of::<u32>() as u32;
        if unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), size) }.is_ok() && cloaked != 0 {
            return BOOL(1);
        }
        let mut class = [0u16; 32];
        let n = unsafe { GetClassNameW(h, &mut class) }.max(0) as usize;
        if matches!(String::from_utf16_lossy(&class[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return BOOL(1);
        }
        let mut r = RECT::default();
        let size = std::mem::size_of::<RECT>() as u32;
        if unsafe { DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, (&mut r as *mut RECT).cast(), size) }.is_err()
            && unsafe { GetWindowRect(h, &mut r) }.is_err()
        {
            return BOOL(1);
        }
        if r.right > r.left && r.bottom > r.top {
            out.push((h.0 as isize, Rect { x: r.left, y: r.top, w: (r.right - r.left) as u32, h: (r.bottom - r.top) as u32 }));
        }
        BOOL(1)
    }
    let mut out = Vec::new();
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize)) };
    out
}

/// Fenêtres d'Oniris retirées (ou remises) des captures. Seulement le temps d'une mesure : les captures d'écran
/// de l'utilisateur voient le panneau le reste du temps.
pub fn exclude(app: &AppHandle, on: bool) {
    for l in ["panel", "pill"] {
        if let Some(w) = app.get_webview_window(l) {
            let _ = w.set_content_protected(on);
        }
    }
}

/// Zone sous la surface du panneau (panel.css : largeur min(1380 px, 96 vw), centrée, en haut).
pub fn region(m: Rect, scale: f64, height_pct: u8) -> Rect {
    let w = ((1380.0 * scale) as u32).min(m.w * 96 / 100);
    let h = if height_pct > 0 { ((m.h as f64 - 28.0 * scale) * height_pct as f64 / 100.0).max(420.0 * scale) as u32 } else { m.h * 4 / 5 };
    Rect { x: m.x + (m.w - w) as i32 / 2, y: m.y, w, h: h.min(m.h) }
}

/// Mesure en continu tant que le panneau est ouvert (arrêtée par `stop`). Si `hidden`, la fenêtre n'est pas encore
/// montrée : l'appelant l'a déjà exclue des captures, la première mesure part sans attendre.
pub fn start(app: &AppHandle, r: Rect, hidden: bool) {
    let me = LOOP.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        let (mut last, mut sig, mut due, mut first) = (None::<Glass>, 0u64, Instant::now(), hidden);
        while LOOP.load(Ordering::SeqCst) == me {
            if app.state::<AppState>().settings.read().glass_mode == "auto" {
                let wins = app_windows();
                let s = signature(r, &wins);
                if Instant::now() >= due || s != sig {
                    (sig, due) = (s, Instant::now() + PERIOD);
                    if !first {
                        exclude(&app, true);
                        std::thread::sleep(SETTLE);
                    }
                    let px = capture(r);
                    exclude(&app, false);
                    first = false;
                    let rects: Vec<Rect> = wins.iter().map(|w| w.1).collect();
                    let g = px.and_then(|p| stats(&p, W)).and_then(|st| next(last, st, cover(r, &rects)));
                    if let (Some(g), true) = (g, LOOP.load(Ordering::SeqCst) == me) {
                        let _ = app.emit("oniris://glass", g);
                        last = Some(g);
                    }
                }
            } else {
                last = None;
                if first {
                    exclude(&app, false);
                    first = false;
                }
            }
            std::thread::sleep(TICK);
        }
        if first {
            exclude(&app, false);
        }
    });
}

pub fn stop() {
    LOOP.fetch_add(1, Ordering::SeqCst);
}

/// Empreinte des fenêtres qui touchent la zone : change quand l'une apparaît, bouge ou disparaît.
fn signature(r: Rect, wins: &[(isize, Rect)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (id, w) in wins.iter().filter(|(_, w)| w.x < r.x + r.w as i32 && r.x < w.x + w.w as i32 && w.y < r.y + r.h as i32 && r.y < w.y + w.h as i32) {
        (id, w.x, w.y, w.w, w.h).hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grille 64 × 36 : `f(x, y, alea)` donne le point en RGB.
    fn grid(f: impl Fn(usize, usize, u32) -> [u8; 3]) -> Vec<u8> {
        let mut seed = 12345u32;
        let mut out = Vec::new();
        for y in 0..H {
            for x in 0..W {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let [r, g, b] = f(x, y, (seed >> 16) % 100);
                out.extend([b, g, r, 255]);
            }
        }
        out
    }

    fn cases() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("noir uni", grid(|_, _, _| [0, 0, 0])),
            // dégradé de nuit, quelques étoiles et reliefs
            ("fond sombre détaillé", grid(|x, y, r| if r < 6 { [200, 210, 230] } else { let v = (20 + x / 3 + y + (r % 15) as usize) as u8; [v / 2, v / 2, v] })),
            ("fond clair", grid(|x, _, _| { let v = 200 + (x / 4) as u8; [v, v, v - 10] })),
            ("page web blanche", grid(|_, _, r| if r < 12 { [30, 30, 30] } else { [255, 255, 255] })),
            ("appli sombre (VS Code)", grid(|x, _, r| if x < 6 { [37, 37, 38] } else if r < 8 { [212, 212, 212] } else if r < 12 { [86, 156, 214] } else { [30, 30, 30] })),
            ("jeu coloré", grid(|x, y, r| match (x / 8 + y / 6 + (r as usize % 2)) % 4 { 0 => [220, 60, 40], 1 => [40, 180, 70], 2 => [250, 200, 40], _ => [60, 90, 210] })),
        ]
    }

    #[test]
    fn texte_lisible_et_ordre_des_cas_types() {
        let mut a = Vec::new();
        for (name, px) in cases() {
            let s = stats(&px, W).unwrap();
            let (b, tone) = base(s);
            println!("{name:24} base {b:.2} ({tone}) high {:.2} detail {:.3} sat {:.2} contraste {:.1}", s.high, s.detail, s.sat, contrast(through(b, s.high)));
            assert!(contrast(through(b, s.high)) >= TARGET - 0.01, "{name}");
            a.push(b);
        }
        let [noir, nuit, clair, web, code, jeu] = a[..] else { unreachable!() };
        assert_eq!(noir, FLOOR);
        assert!(nuit > noir && nuit < 0.6);
        assert!(clair > 0.7 && web >= clair && web <= CEIL);
        assert!(code > nuit && code < clair);
        assert!(jeu > 0.6);
    }

    #[test]
    fn grille_vide_ou_incomplete_refusee() {
        assert!(stats(&[], W).is_none() && stats(&[0; 12], 2).is_none());
    }

    #[test]
    fn hysteresis_sans_oscillation() {
        let s = |high| Stats { high, detail: 0.0, sat: 0.0, bright: 0.0 };
        let g = next(None, s(0.2), 0.0).unwrap();
        assert!(next(Some(g), s(0.21), 0.1).is_none());
        let app = next(Some(g), s(0.2), 0.8).unwrap();
        assert_eq!(app.app, 0.8);
        // l'application reste « derrière » entre 0,2 et 0,4, puis disparaît
        assert_eq!(next(Some(app), s(0.2), 0.3).unwrap().app, 0.3);
        assert_eq!(next(Some(app), s(0.2), 0.1).unwrap().app, 0.0);
        assert!(next(Some(g), s(0.2), 0.3).is_none());
    }

    #[test]
    fn part_couverte() {
        let r = Rect { x: 0, y: 0, w: 1600, h: 900 };
        assert_eq!(cover(r, &[]), 0.0);
        assert_eq!(cover(r, &[Rect { x: -10, y: -10, w: 2000, h: 1000 }]), 1.0);
        assert_eq!(cover(r, &[Rect { x: 0, y: 0, w: 800, h: 900 }]), 0.5);
    }

    #[test]
    fn zone_sous_la_surface() {
        let m = Rect { x: -1920, y: 0, w: 1920, h: 1080 };
        assert_eq!(region(m, 1.0, 0), Rect { x: -1920 + 270, y: 0, w: 1380, h: 864 });
        assert_eq!(region(Rect { x: 0, y: 0, w: 1280, h: 720 }, 1.5, 100).w, 1228);
    }
}
