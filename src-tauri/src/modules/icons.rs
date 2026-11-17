//! Icônes de la palette : celle d'une application de l'inventaire (raccourci, exe ou appli du Store), ou celle du
//! type d'un fichier d'après sa seule extension (le fichier n'est jamais ouvert). PNG 32 px en data URI, gardés en
//! mémoire par clé. Aucune extraction arbitraire : une application doit figurer dans l'inventaire.
use crate::state::AppState;
use base64::Engine;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tauri::State;

#[derive(serde::Serialize, Clone, Default, PartialEq, Debug)]
pub struct Icon {
    /// `data:image/png;base64,…`, absente si Windows n'en donne pas.
    png: Option<String>,
    /// Libellé du type par Windows (« Fichier JSON »), pour les fichiers seulement.
    type_name: Option<String>,
}

static CACHE: LazyLock<Mutex<HashMap<String, Icon>>> = LazyLock::new(Default::default);
/// Garde-fou mémoire : l'inventaire et les extensions restent bien en dessous.
const CACHE_MAX: usize = 4000;

/// Extension sûre (1 à 10 caractères alphanumériques ASCII, minuscules), sinon `None`.
fn ext_of(path: &str) -> Option<String> {
    let e = std::path::Path::new(path).extension()?.to_str()?;
    (!e.is_empty() && e.len() <= 10 && e.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| e.to_ascii_lowercase())
}

/// Icône d'un résultat de la palette : `app` (clé de l'inventaire) ou `file` (extension seulement) ; autre : rien.
#[tauri::command]
pub async fn search_icon(state: State<'_, AppState>, kind: String, key: String) -> Result<Icon, String> {
    let (cache_key, target) = match kind.as_str() {
        "app" if state.apps.contains_key(&key) => (format!("app:{key}"), Target::App(key)),
        "app" => return Err("application inconnue".into()),
        "file" => match ext_of(&key) {
            Some(e) => (format!("ext:{e}"), Target::Ext(e)),
            None => return Ok(Icon::default()),
        },
        _ => return Err("type sans icône".into()),
    };
    if let Some(i) = CACHE.lock().get(&cache_key) {
        return Ok(i.clone());
    }
    let icon = tauri::async_runtime::spawn_blocking(move || extract(&target)).await.map_err(|e| e.to_string())?;
    let mut c = CACHE.lock();
    if c.len() < CACHE_MAX {
        c.insert(cache_key, icon.clone());
    }
    Ok(icon)
}

/// Icône de l'exe d'un processus qui joue du son (chemin donné par Windows, jamais par l'interface).
/// À appeler hors du fil principal : extraction Shell bloquante, mise en cache.
pub(crate) fn exe_icon(path: &str) -> Option<String> {
    let key = format!("exe:{}", path.to_lowercase());
    if let Some(i) = CACHE.lock().get(&key) {
        return i.png.clone();
    }
    let icon = extract(&Target::App(path.to_string()));
    let mut c = CACHE.lock();
    if c.len() < CACHE_MAX {
        c.insert(key, icon.clone());
    }
    icon.png
}

enum Target { App(String), Ext(String) }

fn extract(t: &Target) -> Icon {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    // SAFETY: COM initialisé puis libéré sur ce fil du pool bloquant ; chaque handle est libéré dans `shell_icon`.
    unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let r = shell_icon(t).unwrap_or_default();
        if com {
            CoUninitialize();
        }
        r
    }
}

unsafe fn shell_icon(t: &Target) -> Option<Icon> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES};
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
    let ext = match t {
        Target::App(k) => return Some(Icon { png: app_png(k).map(data_uri), type_name: None }),
        Target::Ext(e) => e,
    };
    let name: Vec<u16> = format!("x.{ext}").encode_utf16().chain(Some(0)).collect();
    let mut sfi = SHFILEINFOW::default();
    let flags = SHGFI_USEFILEATTRIBUTES | SHGFI_ICON | SHGFI_LARGEICON | SHGFI_TYPENAME;
    if SHGetFileInfoW(PCWSTR(name.as_ptr()), FILE_ATTRIBUTE_NORMAL, Some(&mut sfi), std::mem::size_of::<SHFILEINFOW>() as u32, flags) == 0 {
        return None;
    }
    let png = if sfi.hIcon.is_invalid() { None } else {
        let p = icon_png(sfi.hIcon);
        let _ = DestroyIcon(sfi.hIcon);
        p.map(data_uri)
    };
    let n = String::from_utf16_lossy(&sfi.szTypeName);
    let n = n.trim_end_matches('\0').trim();
    Some(Icon { png, type_name: (!n.is_empty()).then(|| n.to_string()) })
}

fn data_uri(png: Vec<u8>) -> String { format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)) }

/// Raccourci, exe ou `shell:AppsFolder\<AppID>` : image 32 px du Shell (alpha prémultiplié, ramené en direct).
unsafe fn app_png(key: &str) -> Option<Vec<u8>> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{DeleteObject, HBITMAP};
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
    let f: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(key), None).ok()?;
    let bmp: HBITMAP = f.GetImage(SIZE { cx: 32, cy: 32 }, SIIGBF_ICONONLY).ok()?;
    let px = bitmap_bgra(bmp);
    let _ = DeleteObject(bmp.into());
    let (mut px, w, h) = px?;
    for p in px.as_chunks_mut::<4>().0.iter_mut() {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] { *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8; }
        }
    }
    to_png(px, w, h)
}

/// Pixels BGRA (haut en premier) d'un bitmap de 256 px au plus.
unsafe fn bitmap_bgra(bmp: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::Graphics::Gdi::*;
    let mut bm = BITMAP::default();
    if GetObjectW(bmp.into(), std::mem::size_of::<BITMAP>() as i32, Some((&mut bm as *mut BITMAP).cast())) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
        return None;
    }
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32,
            biCompression: BI_RGB.0, ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = CreateCompatibleDC(None);
    let n = GetDIBits(dc, bmp, 0, h as u32, Some(px.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
    let _ = DeleteDC(dc);
    (n == h).then_some((px, w as u32, h as u32))
}

/// HICON vers PNG : couleurs BGRA de l'icône ; masque appliqué si elle n'a pas de canal alpha.
unsafe fn icon_png(h: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::DeleteObject;
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};
    let mut ii = ICONINFO::default();
    GetIconInfo(h, &mut ii).ok()?;
    let out = (|| {
        if ii.hbmColor.is_invalid() {
            return None;
        }
        let (mut px, w, hgt) = bitmap_bgra(ii.hbmColor)?;
        if px.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            let (mask, mw, mh) = bitmap_bgra(ii.hbmMask)?;
            if (mw, mh) != (w, hgt) {
                return None;
            }
            for (p, m) in px.as_chunks_mut::<4>().0.iter_mut().zip(mask.as_chunks::<4>().0.iter()) {
                p[3] = if m[0] == 0 { 255 } else { 0 };
            }
        }
        to_png(px, w, hgt)
    })();
    let _ = DeleteObject(ii.hbmColor.into());
    let _ = DeleteObject(ii.hbmMask.into());
    out
}

/// Pixels BGRA (haut en premier) vers PNG RGBA.
fn to_png(mut px: Vec<u8>, w: u32, h: u32) -> Option<Vec<u8>> {
    for p in px.as_chunks_mut::<4>().0.iter_mut() {
        p.swap(0, 2);
    }
    let img = image::RgbaImage::from_raw(w, h, px)?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_seule_et_bornee() {
        assert_eq!(ext_of(r"C:\u\Rapport.PDF").as_deref(), Some("pdf"));
        assert_eq!(ext_of(r"C:\u\data.json").as_deref(), Some("json"));
        assert_eq!(ext_of(r"C:\u\sans"), None);
        assert_eq!(ext_of(r"C:\u\x.a b"), None);
        assert_eq!(ext_of(r"C:\u\x.abcdefghijk"), None);
        assert_eq!(ext_of(r"C:\u\x.é"), None);
    }

    #[test]
    fn png_en_rgba() {
        let png = to_png(vec![10, 20, 30, 255], 1, 1).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(0, 0).0, [30, 20, 10, 255]);
    }
}
