//! `screen.describe` côté Windows : capture GDI de l'écran principal (réduite, en mémoire, fenêtres d'Oniris exclues),
//! modèle de vision local d'Ollama, et son téléchargement depuis les Réglages (`oniris://vision-pull`).
use crate::modules::iris::ensure_ollama;
use crate::state::AppState;
use async_trait::async_trait;
use oniris_core::iris::{BrainError, LocalStatus, Ollama};
use oniris_core::screen::{jpeg_from_bgra, Capture, DescribeTool, OllamaVision, Screen, Vision, DEFAULT_VISION_MODEL, MAX_WIDTH};
use oniris_core::tools::Tool;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();
static PULLING: AtomicBool = AtomicBool::new(false);

/// Appelé par `AppState::open` : l'outil lit les réglages et les fenêtres par l'`AppHandle`.
pub fn bind(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

fn vision_model() -> String {
    APP.get().map_or_else(|| DEFAULT_VISION_MODEL.into(), |a| a.state::<AppState>().settings.read().iris_vision_model.clone())
}

pub fn tool() -> Arc<dyn Tool> {
    Arc::new(DescribeTool { screen: Arc::new(WinScreen), vision: Arc::new(LocalVision(OllamaVision::new())), model: Arc::new(vision_model) })
}

struct LocalVision(OllamaVision);

#[async_trait]
impl Vision for LocalVision {
    async fn status(&self, model: &str) -> LocalStatus {
        ensure_ollama().await;
        self.0.status(model).await
    }
    async fn ask(&self, model: &str, question: &str, jpeg: &[u8]) -> Result<String, BrainError> {
        let r = self.0.ask(model, question, jpeg).await;
        // Demandé pendant un jeu : on rend la mémoire de la carte graphique tout de suite.
        if crate::fullscreen::active() {
            Ollama::new(model, 0).unload(model).await;
        }
        r
    }
}

struct WinScreen;

impl Screen for WinScreen {
    fn fullscreen(&self) -> bool { crate::fullscreen::active() }
    fn capture(&self) -> Result<Capture, String> {
        let oniris_hidden = exclude(true);
        if oniris_hidden {
            std::thread::sleep(Duration::from_millis(150)); // une image composée par DWM sans nos fenêtres
        }
        let px = grab();
        if oniris_hidden {
            exclude(false);
        }
        let (bgra, w, h) = px.ok_or("capture de l'écran impossible")?;
        Ok(Capture { jpeg: jpeg_from_bgra(&bgra, w, h)?, oniris_hidden })
    }
}

/// Exclut (ou réintègre) des captures les fenêtres visibles de notre processus ; vrai si toutes ont suivi.
/// Par Win32 seulement (pas par les fenêtres Tauri, qui lieraient toute la pile de fenêtrage aux tests).
fn exclude(on: bool) -> bool {
    use ::windows::core::BOOL;
    use ::windows::Win32::Foundation::{HWND, LPARAM};
    use ::windows::Win32::System::Threading::GetCurrentProcessId;
    use ::windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};
    unsafe extern "system" fn ours(h: HWND, l: LPARAM) -> BOOL {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        if pid == unsafe { GetCurrentProcessId() } && unsafe { IsWindowVisible(h) }.as_bool() {
            unsafe { &mut *(l.0 as *mut Vec<HWND>) }.push(h);
        }
        BOOL(1)
    }
    let mut found: Vec<HWND> = Vec::new();
    if unsafe { EnumWindows(Some(ours), LPARAM(&mut found as *mut _ as isize)) }.is_err() {
        return false;
    }
    let affinity = if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    // Toutes réglées, même après un échec : jamais de fenêtre laissée exclue.
    found.iter().filter(|h| unsafe { SetWindowDisplayAffinity(**h, affinity) }.is_err()).count() == 0
}

/// Écran principal en BGRA, réduit à `MAX_WIDTH` px de large (lissage HALFTONE).
fn grab() -> Option<(Vec<u8>, u32, u32)> {
    use ::windows::Win32::Graphics::Gdi::*;
    use ::windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe {
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        if sw <= 0 || sh <= 0 {
            return None;
        }
        let w = sw.min(MAX_WIDTH as i32);
        let h = ((sh as i64 * w as i64) / sw as i64).max(1) as i32;
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        SetStretchBltMode(mem, HALFTONE);
        let _ = SetBrushOrgEx(mem, 0, 0, None);
        let ok = StretchBlt(mem, 0, 0, w, h, Some(screen), 0, 0, sw, sh, SRCCOPY | CAPTUREBLT).as_bool();
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
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = if ok { GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS) } else { 0 };
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        (lines == h).then_some((buf, w as u32, h as u32))
    }
}

/// État du modèle de vision (Réglages → IRIS).
#[tauri::command]
pub async fn screen_vision_status() -> (LocalStatus, String) {
    let model = vision_model();
    ensure_ollama().await;
    (OllamaVision::new().status(&model).await, model)
}

#[derive(Serialize, Clone)]
struct PullEvent { done: Option<u64>, total: Option<u64>, error: Option<String> }

struct PullGuard;
impl Drop for PullGuard { fn drop(&mut self) { PULLING.store(false, Ordering::SeqCst); } }

/// Télécharge le modèle de vision réglé (même client que le modèle principal) ; progression sur `oniris://vision-pull`.
#[tauri::command]
pub async fn screen_vision_pull(app: AppHandle) -> Result<(), String> {
    if PULLING.swap(true, Ordering::SeqCst) {
        return Err("téléchargement déjà en cours".into());
    }
    let _guard = PullGuard;
    ensure_ollama().await;
    let model = vision_model();
    let emit = |done, total, error| { let _ = app.emit("oniris://vision-pull", PullEvent { done, total, error }); };
    let mut last = 0;
    match Ollama::new(&model, 0).pull(&model, |d, t| { last = t; emit(Some(d), Some(t), None); }).await {
        Ok(()) => { emit(Some(last), Some(last), None); Ok(()) }
        Err(e) => { tracing::warn!("oniris: téléchargement du modèle de vision échoué ({e})"); emit(None, None, Some(e.to_string())); Err(e.to_string()) }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn jamais_offert_a_claude() {
        assert!(crate::modules::iris_mcp::EXCLUDED.contains(&"screen.describe"));
        assert_eq!(super::tool().spec().name, "screen.describe");
    }
}
