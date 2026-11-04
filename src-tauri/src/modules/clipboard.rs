//! Historique du presse-papiers côté Windows : écoute sur une fenêtre message-only, conversion DIB → PNG,
//! commandes et collage.
//!
//! Le thread d'écoute ouvre, lit et referme le presse-papiers au plus vite ; l'écriture en base et l'encodage
//! PNG partent dans un canal vers un second thread. Nos propres écritures portent le format `Oniris.Self` et
//! sont ignorées. Journal : type, taille et id seulement, jamais le contenu.
//! R8 : rien ici ne touche le vrai presse-papiers pendant les tests ; seules les fonctions pures sont testées.
use crate::hotkey::marked_key;
use crate::state::AppState;
use ::windows::core::w;
use ::windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use ::windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use ::windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_UNICODETEXT};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, RegisterClassW,
    SetForegroundWindow, HWND_MESSAGE, MSG,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW,
};
use base64::Engine;
use chrono::Local;
use image::{imageops, ImageFormat, RgbaImage};
use oniris_core::clipboard::{self, should_record, ClipContent, ClipEntry, ClipFlags, MAX_TEXT_BYTES};
use oniris_core::events::Topic;
use serde::Serialize;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

/// Au-delà, une image n'est pas enregistrée.
const MAX_IMAGE_BYTES: usize = 40 * 1024 * 1024; // une capture 4K en 32 bits fait 33 Mo
/// Miniature : dans un carré de 160 px, proportions gardées.
const THUMB_MAX: u32 = 160;
const LIST_LIMIT: usize = 50;
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

static APP: OnceLock<AppHandle> = OnceLock::new();
static TX: OnceLock<Sender<Captured>> = OnceLock::new();
/// Fenêtre d'écoute (HWND en entier) : elle sert aussi de propriétaire à nos écritures.
static LISTENER: AtomicIsize = AtomicIsize::new(0);

/// Lu sur le thread d'écoute, traité sur le thread d'enregistrement.
enum Captured { Text(String), Dib(Vec<u8>) }

/// Entrée envoyée au panneau : miniature PNG en base64 (affichée en `data:image/png;base64`).
#[derive(Serialize)]
pub struct ClipItem {
    id: i64,
    kind: String,
    text: Option<String>,
    truncated: bool,
    thumb: Option<String>,
    pinned: bool,
    created_ms: i64,
}

impl From<ClipEntry> for ClipItem {
    fn from(e: ClipEntry) -> Self {
        let thumb = e.thumb.map(|t| base64::engine::general_purpose::STANDARD.encode(t));
        Self { id: e.id, kind: e.kind, text: e.text, truncated: e.truncated, thumb, pinned: e.pinned, created_ms: e.created_ms }
    }
}

// ---------- Fonctions pures (testées) ----------

/// DIB (`CF_DIB` / `CF_DIBV5` : en-tête puis pixels) vers (PNG, miniature PNG dans 160×160, proportions gardées).
/// 24 et 32 bits, BI_RGB et BI_BITFIELDS, de bas en haut ou de haut en bas. Alpha 32 bits tout à zéro : opaque.
pub fn dib_to_png(dib: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if dib.len() > MAX_IMAGE_BYTES {
        return Err(format!("image de plus de 40 Mo ({} octets)", dib.len()));
    }
    let u32_at = |o: usize| dib.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("en-tête DIB tronqué");
    let header = u32_at(0)? as usize;
    let (width, height) = (u32_at(4)? as i32, u32_at(8)? as i32);
    let bpp = u32_at(12)? >> 16;
    let (compression, colors) = (u32_at(16)?, u32_at(32)? as usize);
    if header < 40 || width <= 0 || height == 0 || height == i32::MIN {
        return Err(format!("en-tête DIB invalide ({header} octets, {width}×{height})"));
    }
    // Masques : dans l'en-tête à partir de V2 (52 octets, alpha dès V3 à 56), sinon juste après l'en-tête de 40.
    let (masks, mut offset) = match (bpp, compression) {
        (24 | 32, BI_RGB) => (None, header),
        (32, BI_BITFIELDS) if header == 40 => (Some([u32_at(40)?, u32_at(44)?, u32_at(48)?, 0]), 52),
        (32, BI_BITFIELDS) => (Some([u32_at(40)?, u32_at(44)?, u32_at(48)?, if header >= 56 { u32_at(52)? } else { 0 }]), header),
        _ => return Err(format!("DIB non pris en charge ({bpp} bits, compression {compression})")),
    };
    offset += colors * 4;
    let (w, h, bytes_pp) = (width as usize, height.unsigned_abs() as usize, bpp as usize / 8);
    let stride = (w * bytes_pp).div_ceil(4) * 4;
    let data = stride.checked_mul(h).and_then(|n| dib.get(offset..offset.checked_add(n)?)).ok_or("pixels DIB tronqués")?;
    let chan = |v: u32, m: u32| if m == 0 { 0 } else { ((u64::from((v & m) >> m.trailing_zeros()) * 255) / u64::from(m >> m.trailing_zeros())) as u8 };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = if height < 0 { y } else { h - 1 - y };
        for px in data[row * stride..row * stride + w * bytes_pp].chunks_exact(bytes_pp) {
            match masks {
                None => rgba.extend([px[2], px[1], px[0], if bpp == 32 { px[3] } else { 255 }]),
                Some([r, g, b, a]) => {
                    let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                    rgba.extend([chan(v, r), chan(v, g), chan(v, b), if a == 0 { 255 } else { chan(v, a) }]);
                }
            }
        }
    }
    if rgba.iter().skip(3).step_by(4).all(|&a| a == 0) {
        rgba.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
    }
    let img = RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("dimensions DIB incohérentes")?;
    let png = encode_png(&img)?;
    let (iw, ih) = img.dimensions();
    let thumb = if iw > THUMB_MAX || ih > THUMB_MAX {
        let fit = |a: u32, b: u32| (u64::from(a) * u64::from(THUMB_MAX) / u64::from(b)).max(1) as u32;
        let (tw, th) = if iw >= ih { (THUMB_MAX, fit(ih, iw)) } else { (fit(iw, ih), THUMB_MAX) };
        encode_png(&imageops::resize(&img, tw, th, imageops::FilterType::Triangle))?
    } else {
        png.clone()
    };
    Ok((png, thumb))
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// PNG vers DIB 32 bits BI_RGB de bas en haut (en-tête de 40 octets), pour `SetClipboardData(CF_DIB)`.
fn png_to_dib(png: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory_with_format(png, ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut out = Vec::with_capacity(40 + img.as_raw().len());
    out.extend(40u32.to_le_bytes());
    out.extend((w as i32).to_le_bytes());
    out.extend((h as i32).to_le_bytes()); // positif : de bas en haut, le plus compatible
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(BI_RGB.to_le_bytes());
    out.extend((w * h * 4).to_le_bytes());
    out.extend([0u8; 16]);
    for row in img.rows().rev() {
        row.for_each(|p| out.extend([p[2], p[1], p[0], p[3]]));
    }
    Ok(out)
}

/// `CF_UNICODETEXT` (UTF-16LE terminé par NUL) vers texte. Lit au plus `MAX_TEXT_BYTES + 1` unités : assez pour
/// que `truncate_text` sache que le texte déborde, sans copier 10 Mo presse-papiers ouvert.
fn utf16_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).take_while(|&u| u != 0).take(MAX_TEXT_BYTES + 1).collect();
    String::from_utf16_lossy(&units)
}

// ---------- Commandes ----------

/// Requête vide : entrées récentes ; sinon textes contenant la requête.
#[tauri::command]
pub fn clipboard_list(state: State<AppState>, query: String) -> Result<Vec<ClipItem>, String> {
    let list = clipboard::search(&state.db.lock(), &query, LIST_LIMIT).map_err(|e| e.to_string())?;
    Ok(list.into_iter().map(ClipItem::from).collect())
}

// Épingler et supprimer écrivent directement : par le courtier, le journal des actions garderait le contenu
// (undo d'une suppression) hors de portée de la purge.
#[tauri::command]
pub fn clipboard_pin(state: State<AppState>, id: i64, pinned: bool) -> Result<(), String> {
    clipboard::set_pinned(&state.db.lock(), id, pinned).map_err(|e| e.to_string())?;
    state.events.changed(Topic::Clipboard);
    Ok(())
}

#[tauri::command]
pub fn clipboard_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    clipboard::delete(&state.db.lock(), id).map_err(|e| e.to_string())?;
    tracing::info!("presse-papiers : entrée {id} supprimée");
    state.events.changed(Topic::Clipboard);
    Ok(())
}

/// Remet l'entrée dans le presse-papiers (marquée `Oniris.Self`), ferme le panneau, puis après 80 ms rend le
/// premier plan à la fenêtre d'avant l'ouverture et y colle. Hors du thread principal (ouverture du presse-papiers).
#[tauri::command(async)]
pub fn clipboard_paste(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let content = clipboard::get(&state.db.lock(), id).map_err(|e| e.to_string())?.ok_or("entrée introuvable")?;
    write_clipboard(&content)?;
    tracing::info!("presse-papiers : entrée {id} remise pour collage");
    let prev = state.prev_foreground.load(Ordering::SeqCst);
    crate::windows::close_panel(&app).map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        paste_into(prev);
    });
    Ok(())
}

// ---------- Démarrage, écoute et enregistrement ----------

/// À appeler dans `setup`, état géré : purge, thread d'enregistrement, puis thread d'écoute.
pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    purge_now(&app.state::<AppState>());
    let (tx, rx) = channel();
    let _ = TX.set(tx);
    let h = app.clone();
    std::thread::spawn(move || record_loop(h, rx));
    std::thread::spawn(listen);
}

fn purge_now(state: &AppState) {
    let (days, max) = { let s = state.settings.read(); (s.clipboard_retention_days, s.clipboard_max_items) };
    match clipboard::purge(&state.db.lock(), Local::now().timestamp_millis(), days, max) {
        Ok(0) => {}
        Ok(n) => tracing::info!("presse-papiers : {n} entrées purgées"),
        Err(e) => tracing::error!("presse-papiers : purge impossible : {e}"),
    }
}

fn record_loop(app: AppHandle, rx: Receiver<Captured>) {
    for cap in rx {
        let state = app.state::<AppState>();
        let now = Local::now().timestamp_millis();
        let res = match cap {
            Captured::Text(t) if t.trim().is_empty() => continue,
            Captured::Text(t) => clipboard::record_text(&state.db.lock(), &t, now).map(|id| ("texte", t.len(), id)),
            Captured::Dib(d) => match dib_to_png(&d) {
                Ok((png, thumb)) => clipboard::record_image(&state.db.lock(), &png, &thumb, now).map(|id| ("image", png.len(), id)),
                Err(e) => { tracing::warn!("presse-papiers : image de {} octets ignorée : {e}", d.len()); continue; }
            },
        };
        match res {
            Ok((kind, len, id)) => tracing::info!("presse-papiers : {kind} de {len} octets, entrée {id}"),
            Err(e) => { tracing::error!("presse-papiers : enregistrement impossible : {e}"); continue; }
        }
        purge_now(&state);
        state.events.changed(Topic::Clipboard);
    }
}

struct Formats { own: u32, exclude: u32, history: u32, cloud: u32 }

/// `None` si un format n'a pas pu être enregistré : plus rien n'est lu ni écrit (échec fermé, journalisé une fois).
fn formats() -> Option<&'static Formats> {
    static F: OnceLock<Option<Formats>> = OnceLock::new();
    F.get_or_init(|| {
        let f = unsafe {
            Formats {
                own: RegisterClipboardFormatW(w!("Oniris.Self")),
                exclude: RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
                history: RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                cloud: RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
            }
        };
        if [f.own, f.exclude, f.history, f.cloud].contains(&0) {
            tracing::error!("presse-papiers : formats d'exclusion non enregistrés, historique désactivé");
            return None;
        }
        Some(f)
    })
    .as_ref()
}

fn listen() {
    unsafe {
        let hinst = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => return tracing::error!("presse-papiers : module introuvable : {e}"),
        };
        let class = w!("OnirisClipboardListener");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
        if RegisterClassW(&wc) == 0 {
            return tracing::error!("presse-papiers : classe de fenêtre refusée");
        }
        let hwnd = match CreateWindowExW(WINDOW_EX_STYLE::default(), class, w!(""), WINDOW_STYLE::default(), 0, 0, 0, 0,
            Some(HWND_MESSAGE), None, Some(hinst.into()), None) {
            Ok(h) => h,
            Err(e) => return tracing::error!("presse-papiers : fenêtre d'écoute impossible : {e}"),
        };
        if let Err(e) = AddClipboardFormatListener(hwnd) {
            return tracing::error!("presse-papiers : écoute refusée : {e}");
        }
        LISTENER.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = formats();
        let mut msg = MSG::default();
        // -1 (erreur) arrête la boucle au lieu de la faire tourner à vide.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        on_update(hwnd);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Réglage relu à chaque copie : désactivé, rien n'est lu.
fn on_update(hwnd: HWND) {
    let Some(app) = APP.get() else { return };
    if !app.state::<AppState>().settings.read().clipboard_enabled {
        return;
    }
    if !open_clipboard(hwnd) {
        return tracing::warn!("presse-papiers : verrouillé par une autre application, copie ignorée");
    }
    let cap = unsafe { read_open_clipboard() };
    let _ = unsafe { CloseClipboard() };
    if let (Some(cap), Some(tx)) = (cap, TX.get()) {
        let _ = tx.send(cap);
    }
}

/// Premier essai puis 2 nouvelles tentatives à 50 ms d'intervalle (presse-papiers tenu par une autre app).
fn open_clipboard(hwnd: HWND) -> bool {
    (0..3).any(|i| {
        if i > 0 { std::thread::sleep(Duration::from_millis(50)); }
        unsafe { OpenClipboard(Some(hwnd)) }.is_ok()
    })
}

/// Octets d'un format pendant son verrou global ; `None` si absent ou illisible.
unsafe fn with_data<T>(fmt: u32, f: impl FnOnce(&[u8]) -> Option<T>) -> Option<T> {
    let h = unsafe { GetClipboardData(fmt) }.ok()?;
    let g = HGLOBAL(h.0);
    let p = unsafe { GlobalLock(g) } as *const u8;
    if p.is_null() {
        return None;
    }
    let out = f(unsafe { std::slice::from_raw_parts(p, GlobalSize(g)) });
    let _ = unsafe { GlobalUnlock(g) };
    out
}

/// Presse-papiers déjà ouvert. Exclusions d'abord : un format présent mais illisible vaut « ne pas garder ».
unsafe fn read_open_clipboard() -> Option<Captured> {
    let f = formats()?;
    let has = |fmt: u32| unsafe { IsClipboardFormatAvailable(fmt) }.is_ok();
    if has(f.own) {
        return None;
    }
    let dword = |fmt: u32| has(fmt).then(|| unsafe {
        with_data(fmt, |b| b.get(..4).map(|d| u32::from_le_bytes([d[0], d[1], d[2], d[3]]))).unwrap_or(0)
    });
    let flags = ClipFlags { exclude_monitor: has(f.exclude), history_allowed: dword(f.history), cloud_allowed: dword(f.cloud) };
    if !should_record(&flags) {
        tracing::info!("presse-papiers : copie exclue par sa source");
        return None;
    }
    if has(CF_UNICODETEXT.0.into()) {
        return unsafe { with_data(CF_UNICODETEXT.0.into(), |b| Some(Captured::Text(utf16_text(b)))) };
    }
    let fmt = [CF_DIBV5, CF_DIB].into_iter().map(|c| u32::from(c.0)).find(|&c| has(c))?;
    unsafe {
        with_data(fmt, |b| {
            if b.len() > MAX_IMAGE_BYTES {
                tracing::warn!("presse-papiers : image de {} octets ignorée (plus de 40 Mo)", b.len());
                return None;
            }
            Some(Captured::Dib(b.to_vec()))
        })
    }
}

/// Écrit le contenu et le marqueur `Oniris.Self` (notre écoute ignore alors la mise à jour).
fn write_clipboard(content: &ClipContent) -> Result<(), String> {
    let (fmt, bytes) = match content {
        ClipContent::Text(t) => (u32::from(CF_UNICODETEXT.0), t.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()),
        ClipContent::Image(png) => (u32::from(CF_DIB.0), png_to_dib(png)?),
    };
    let own = formats().ok_or("formats du presse-papiers indisponibles")?.own;
    let hwnd = HWND(LISTENER.load(Ordering::SeqCst) as *mut _);
    if hwnd.0.is_null() {
        return Err("écoute du presse-papiers absente".into());
    }
    if !open_clipboard(hwnd) {
        return Err("presse-papiers verrouillé par une autre application".into());
    }
    let res = unsafe {
        EmptyClipboard().map_err(|e| e.to_string())
            .and_then(|_| set_data(fmt, &bytes))
            .and_then(|_| set_data(own, &[1, 0, 0, 0]))
    };
    let _ = unsafe { CloseClipboard() };
    res
}

/// Copie dans un bloc global confié au presse-papiers ; libéré ici seulement s'il est refusé.
unsafe fn set_data(fmt: u32, bytes: &[u8]) -> Result<(), String> {
    let g = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.map_err(|e| e.to_string())?;
    let p = unsafe { GlobalLock(g) } as *mut u8;
    if p.is_null() {
        let _ = unsafe { GlobalFree(Some(g)) };
        return Err("mémoire globale indisponible".into());
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len()) };
    let _ = unsafe { GlobalUnlock(g) };
    if let Err(e) = unsafe { SetClipboardData(fmt, Some(HANDLE(g.0))) } {
        let _ = unsafe { GlobalFree(Some(g)) };
        return Err(e.to_string());
    }
    Ok(())
}

/// Coller seulement si la fenêtre d'origine est connue et de nouveau au premier plan.
fn should_inject(prev: isize, fg_now: isize) -> bool {
    prev != 0 && prev == fg_now
}

/// Code produit seulement, jamais appelé par les tests (R8). Rend le premier plan à `prev` et attend 100 ms au
/// plus qu'il l'ait ; sinon pas de Ctrl+V (le contenu reste dans le presse-papiers, à coller à la main).
fn paste_into(prev: isize) {
    if prev == 0 {
        return tracing::info!("presse-papiers : fenêtre d'origine inconnue, collage non envoyé");
    }
    let _ = unsafe { SetForegroundWindow(HWND(prev as *mut _)) };
    let deadline = Instant::now() + Duration::from_millis(100);
    while !should_inject(prev, unsafe { GetForegroundWindow() }.0 as isize) {
        if Instant::now() >= deadline {
            return tracing::info!("presse-papiers : fenêtre d'origine pas revenue au premier plan, collage non envoyé");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    send_ctrl_v();
}

/// Code produit seulement, jamais appelé par les tests (R8) : Ctrl+V marqué vers la fenêtre au premier plan.
fn send_ctrl_v() {
    let down = KEYBD_EVENT_FLAGS(0);
    let inputs = [
        marked_key(VK_CONTROL.0, down),
        marked_key(VK_V.0, down),
        marked_key(VK_V.0, KEYEVENTF_KEYUP),
        marked_key(VK_CONTROL.0, KEYEVENTF_KEYUP),
    ];
    let size = std::mem::size_of::<INPUT>() as i32;
    let sent = unsafe { SendInput(&inputs, size) };
    if sent as usize != inputs.len() {
        tracing::warn!("presse-papiers : Ctrl+V injecté en partie ({sent}/4), touches relâchées");
        // Ctrl ne doit jamais rester enfoncé.
        unsafe { SendInput(&[marked_key(VK_V.0, KEYEVENTF_KEYUP), marked_key(VK_CONTROL.0, KEYEVENTF_KEYUP)], size) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Px = [u8; 4]; // RGBA
    const R: Px = [255, 0, 0, 255];
    const G: Px = [0, 255, 0, 255];
    const B: Px = [0, 0, 255, 255];
    const W: Px = [255, 255, 255, 255];

    /// DIB 2×2 synthétique ; `rows` de haut en bas. `header` : 40 (INFO) ou 124 (V5). Masques après l'en-tête de 40.
    fn dib(bpp: u16, top_down: bool, compression: u32, header: u32, rows: [[Px; 2]; 2]) -> Vec<u8> {
        let h: i32 = if top_down { -2 } else { 2 };
        let mut v = Vec::new();
        v.extend(header.to_le_bytes());
        v.extend(2i32.to_le_bytes());
        v.extend(h.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(bpp.to_le_bytes());
        v.extend(compression.to_le_bytes());
        v.extend([0u8; 20]); // taille, résolutions, couleurs
        let masks = [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000];
        if header > 40 {
            for m in masks { v.extend(m.to_le_bytes()); }
            v.resize(header as usize, 0);
        } else if compression == BI_BITFIELDS {
            for m in &masks[..3] { v.extend(m.to_le_bytes()); }
        }
        let order: Vec<&[Px; 2]> = if top_down { rows.iter().collect() } else { rows.iter().rev().collect() };
        for row in order {
            let start = v.len();
            for p in row {
                v.extend([p[2], p[1], p[0]]);
                if bpp == 32 { v.push(p[3]); }
            }
            while (v.len() - start) % 4 != 0 { v.push(0); }
        }
        v
    }

    fn pixels(png: &[u8]) -> (u32, u32, Vec<Px>) {
        let img = image::load_from_memory_with_format(png, ImageFormat::Png).unwrap().to_rgba8();
        (img.width(), img.height(), img.pixels().map(|p| p.0).collect())
    }

    const ROWS: [[Px; 2]; 2] = [[R, G], [B, W]];

    #[test]
    fn dib_24_bits_bas_en_haut_et_haut_en_bas() {
        for top_down in [false, true] {
            let (png, thumb) = dib_to_png(&dib(24, top_down, BI_RGB, 40, ROWS)).unwrap();
            assert_eq!(pixels(&png), (2, 2, vec![R, G, B, W]), "top_down = {top_down}");
            assert_eq!(pixels(&thumb).0, 2, "petite image : miniature à la même taille");
        }
    }

    #[test]
    fn dib_32_bits_alpha_nul_devient_opaque() {
        let zero = ROWS.map(|r| r.map(|[r, g, b, _]| [r, g, b, 0]));
        for top_down in [false, true] {
            let (png, _) = dib_to_png(&dib(32, top_down, BI_RGB, 40, zero)).unwrap();
            assert_eq!(pixels(&png), (2, 2, vec![R, G, B, W]), "top_down = {top_down}");
        }
    }

    #[test]
    fn dib_32_bits_alpha_garde() {
        let half = [[[255, 0, 0, 128], G], [B, [255, 255, 255, 0]]];
        let (png, _) = dib_to_png(&dib(32, false, BI_RGB, 40, half)).unwrap();
        assert_eq!(pixels(&png).2, vec![[255, 0, 0, 128], G, B, [255, 255, 255, 0]]);
    }

    #[test]
    fn dib_32_bits_bitfields_info_et_v5() {
        let (png, _) = dib_to_png(&dib(32, false, BI_BITFIELDS, 40, ROWS)).unwrap();
        assert_eq!(pixels(&png).2, vec![R, G, B, W]);
        let half = [[[255, 0, 0, 128], G], [B, W]];
        let (png, _) = dib_to_png(&dib(32, true, BI_BITFIELDS, 124, half)).unwrap();
        assert_eq!(pixels(&png).2, vec![[255, 0, 0, 128], G, B, W], "masque alpha de l'en-tête V5");
    }

    #[test]
    fn dib_refuse_profondeur_inconnue_tronque_et_trop_lourd() {
        assert!(dib_to_png(&dib(16, false, BI_RGB, 40, ROWS)).is_err());
        assert!(dib_to_png(&dib(24, false, BI_BITFIELDS, 40, ROWS)).is_err());
        let d = dib(32, false, BI_RGB, 40, ROWS);
        assert!(dib_to_png(&d[..d.len() - 1]).is_err());
        assert!(dib_to_png(&d[..20]).is_err());
        let mut big = d.clone();
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        assert!(dib_to_png(&big).is_err());
    }

    /// DIB 32 bits de haut en bas, `w`×`h`, tous les pixels identiques.
    fn plain(w: u32, h: u32) -> Vec<u8> {
        let mut d = dib(32, true, BI_RGB, 40, ROWS);
        d.truncate(40);
        d[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        d[8..12].copy_from_slice(&(-(h as i32)).to_le_bytes());
        d.resize(40 + (w * h * 4) as usize, 200);
        d
    }

    #[test]
    fn miniature_dans_160_par_160_proportions_gardees() {
        for ((w, h), want) in [((400, 10), (160, 4)), ((10, 400), (4, 160)), ((320, 640), (80, 160)), ((150, 100), (150, 100))] {
            let (png, thumb) = dib_to_png(&plain(w, h)).unwrap();
            assert_eq!((pixels(&png).0, pixels(&png).1), (w, h));
            let (tw, th, _) = pixels(&thumb);
            assert_eq!((tw, th), want, "{w}×{h}");
        }
    }

    #[test]
    fn capture_4k_gardee_sous_le_plafond_de_40_mo() {
        assert_eq!(MAX_IMAGE_BYTES, 40 * 1024 * 1024);
        let d = plain(3840, 2160); // 33 Mo en 32 bits
        assert!(d.len() > 20 * 1024 * 1024 && d.len() <= MAX_IMAGE_BYTES);
        let (_, thumb) = dib_to_png(&d).unwrap();
        assert_eq!((pixels(&thumb).0, pixels(&thumb).1), (160, 90));
    }

    #[test]
    fn injection_seulement_dans_la_fenetre_d_origine() {
        assert!(should_inject(42, 42));
        assert!(!should_inject(42, 43), "autre fenêtre au premier plan");
        assert!(!should_inject(0, 0), "fenêtre d'origine inconnue");
        assert!(!should_inject(0, 42));
    }

    #[test]
    fn png_vers_dib_aller_retour() {
        let half = [[[255, 0, 0, 128], G], [B, W]];
        let (png, _) = dib_to_png(&dib(24, false, BI_RGB, 40, ROWS)).unwrap();
        let (back, _) = dib_to_png(&png_to_dib(&png).unwrap()).unwrap();
        assert_eq!(pixels(&back), (2, 2, vec![R, G, B, W]));
        let (png, _) = dib_to_png(&dib(32, true, BI_RGB, 40, half)).unwrap();
        let (back, _) = dib_to_png(&png_to_dib(&png).unwrap()).unwrap();
        assert_eq!(pixels(&back).2, vec![[255, 0, 0, 128], G, B, W]);
        assert!(png_to_dib(b"pas un png").is_err());
    }

    #[test]
    fn texte_utf16_jusqu_au_nul_et_borne() {
        let enc = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        let mut b = enc("héllo 🙂");
        b.extend([0, 0, b'x', 0]);
        assert_eq!(utf16_text(&b), "héllo 🙂");
        assert_eq!(utf16_text(&enc("sans nul")), "sans nul");
        let big = enc(&"a".repeat(MAX_TEXT_BYTES + 10));
        let t = utf16_text(&big);
        assert_eq!(t.len(), MAX_TEXT_BYTES + 1);
        assert!(clipboard::truncate_text(&t).1, "assez lu pour savoir que ça déborde");
    }
}
