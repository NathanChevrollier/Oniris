//! Exécutions réelles des outils généraux d'IRIS (jamais appelées par les tests hors dossier temporaire : R8).
use async_trait::async_trait;
use base64::Engine;
use oniris_core::general::recycle::{restore_from, Recycler};
use oniris_core::general::shell::{ShellExec, ShellOutput, MAX_OUTPUT};
use oniris_core::general::windows::WindowControl;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

/// Sortie en UTF-8 et sans barre de progression ; l'enveloppe ci-dessous fusionne stderr en texte (sinon CLIXML).
const PRELUDE: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;$ProgressPreference='SilentlyContinue';";

/// Enveloppe : sortie texte, puis code de sortie natif (`$LASTEXITCODE`) ou 1 si la dernière commande a échoué.
const WRAP_HEAD: &str = "$global:LASTEXITCODE=0;& { ";
const WRAP_TAIL: &str = "$global:oniris_ok = $?\n} 2>&1 | Out-String -Width 4096; if ($global:LASTEXITCODE) { exit $global:LASTEXITCODE }; if (-not $global:oniris_ok) { exit 1 }";

pub struct RealShell;

/// Garde au plus `MAX_OUTPUT` octets puis vide le tube (le processus ne doit jamais bloquer sur un tube plein).
async fn read_capped(mut r: impl AsyncRead + Unpin) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = (&mut r).take(MAX_OUTPUT as u64).read_to_end(&mut buf).await;
    let _ = tokio::io::copy(&mut r, &mut tokio::io::sink()).await;
    buf
}

#[async_trait]
impl ShellExec for RealShell {
    async fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> Result<ShellOutput, String> {
        let utf16: Vec<u8> = format!("{PRELUDE}{WRAP_HEAD}{command}\n{WRAP_TAIL}").encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        let root = std::env::var_os("SystemRoot").ok_or("SystemRoot absente")?;
        let exe = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let mut cmd = Command::new(exe);
        cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .current_dir(cwd)
            .env("GIT_PAGER", "cat")
            .env("PAGER", "")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.fsmonitor")
            .env("GIT_CONFIG_VALUE_0", "false")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, job) = crate::job::spawn(cmd).map_err(|e| format!("lancement impossible : {e}"))?;
        let (out, err) = (child.stdout.take().ok_or("stdout")?, child.stderr.take().ok_or("stderr")?);
        let work = async { tokio::join!(child.wait(), read_capped(out), read_capped(err)) };
        match tokio::time::timeout(timeout, work).await {
            Ok((status, mut o, e)) => {
                o.extend(e);
                let mut text = String::from_utf8_lossy(&o).into_owned();
                if text.len() > MAX_OUTPUT {
                    let cut = (0..=MAX_OUTPUT).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
                    text.truncate(cut);
                }
                drop(job); // tue d'éventuels descendants restés vivants
                Ok(ShellOutput { exit_code: status.map_err(|e| e.to_string())?.code(), output: text, timed_out: false })
            }
            Err(_) => {
                drop(job);
                Ok(ShellOutput { exit_code: None, output: String::new(), timed_out: true })
            }
        }
    }
}

/// Le processus Oniris a-t-il un jeton élevé ? (en cas d'échec : oui, par prudence)
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: jeton ouvert puis fermé ici ; `e` et `n` vivent pendant l'appel.
    unsafe {
        let mut tok = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok).is_err() {
            return true;
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut n = 0u32;
        let r = GetTokenInformation(tok, TokenElevation, Some(&mut e as *mut _ as *mut _), size_of::<TOKEN_ELEVATION>() as u32, &mut n);
        let _ = CloseHandle(tok);
        r.is_err() || e.TokenIsElevated != 0
    }
}

/// Corbeille du Shell. `FOF_WANTNUKEWARNING` s'ajoute aux drapeaux silencieux : si l'élément ne peut pas aller à la
/// Corbeille (lecteur sans Corbeille, trop gros), Windows demande au lieu de supprimer définitivement sans rien dire.
pub struct ShellRecycler;

impl Recycler for ShellRecycler {
    fn recycle(&self, p: &Path) -> Result<(), String> {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW};
        let from: Vec<u16> = p.as_os_str().encode_wide().chain([0, 0]).collect(); // liste terminée par deux zéros
        let flags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING;
        let mut op = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()), fFlags: flags.0 as u16, ..Default::default() };
        // SAFETY: `from` vit pendant l'appel ; les autres champs sont nuls.
        let r = unsafe { SHFileOperationW(&mut op) };
        if r != 0 || op.fAnyOperationsAborted.as_bool() {
            return Err(format!("Corbeille : échec (code {r})"));
        }
        Ok(())
    }
    fn restore(&self, original: &Path, deleted_after_ms: i64) -> Result<(), String> {
        let drive = original.to_string_lossy().get(..2).ok_or("lecteur inconnu")?.to_string();
        restore_from(&Path::new(&format!(r"{drive}\$Recycle.Bin")).join(user_sid()?), original, deleted_after_ms)
    }
}

/// SID de l'utilisateur courant (`S-1-5-21-…`) : nom de son dossier dans `$Recycle.Bin`.
fn user_sid() -> Result<String, String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: jeton fermé ici ; `buf` (aligné, 512 octets ≥ TOKEN_USER + SID) vit tant qu'on lit `tu` ; chaîne libérée par LocalFree.
    unsafe {
        let mut tok = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok).map_err(|e| e.to_string())?;
        let mut buf = [0u64; 64];
        let mut n = 0u32;
        let r = GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr().cast()), size_of_val(&buf) as u32, &mut n);
        let _ = CloseHandle(tok);
        r.map_err(|e| e.to_string())?;
        let tu = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(tu.User.Sid, &mut s).map_err(|e| e.to_string())?;
        let out = s.to_string().map_err(|e| e.to_string());
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}

/// Fenêtres des applications de l'inventaire : celles des processus dont l'image est la cible du raccourci.
// shortcut: un raccourci vers un lanceur (Update.exe…) ne retrouve pas le processus réel ; à affiner si besoin.
pub struct Win32Windows;

/// Programme visé par un raccourci `.lnk` (lu sans résolution : sa cible n'est jamais touchée).
fn lnk_target(lnk: &str) -> Result<PathBuf, String> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    // SAFETY: COM initialisé (et rendu) sur ce fil ; `buf` vit pendant `GetPath`.
    let buf = unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let r = (|| -> windows::core::Result<Vec<u16>> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.cast::<IPersistFile>()?.Load(&HSTRING::from(lnk), STGM_READ)?;
            let mut buf = vec![0u16; 4096];
            link.GetPath(&mut buf, std::ptr::null_mut(), 0)?;
            Ok(buf)
        })();
        if init {
            CoUninitialize();
        }
        r.map_err(|e| format!("raccourci illisible : {e}"))?
    };
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if n == 0 {
        return Err("raccourci sans programme cible".into());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

/// Image (chemin complet) du processus `pid`.
fn image(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: poignée fermée ici ; `buf` vit pendant l'appel et `n` en borne la lecture.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// Fenêtres principales visibles (sans propriétaire) des processus dont l'image est `exe` (casse ignorée).
fn windows_of(exe: &Path) -> Vec<windows::Win32::Foundation::HWND> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER};
    unsafe extern "system" fn push(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: `l` pointe le Vec ci-dessous, vivant pendant `EnumWindows`.
        unsafe { (*(l.0 as *mut Vec<HWND>)).push(h) };
        true.into()
    }
    let mut all: Vec<HWND> = vec![];
    // SAFETY: rappel synchrone ; `all` survit à l'appel.
    let _ = unsafe { EnumWindows(Some(push), LPARAM(&mut all as *mut _ as isize)) };
    let (me, want) = (std::process::id(), exe.to_string_lossy().to_lowercase());
    all.into_iter()
        .filter(|&h| {
            let mut pid = 0;
            // SAFETY: simples lectures sur une fenêtre (une fenêtre disparue rend 0 ou une erreur).
            let main = unsafe { IsWindowVisible(h).as_bool() && GetWindow(h, GW_OWNER).is_err() && GetWindowThreadProcessId(h, Some(&mut pid)) != 0 };
            main && pid != me && image(pid).is_some_and(|p| p.to_lowercase() == want)
        })
        .collect()
}

impl WindowControl for Win32Windows {
    fn focus(&self, app_key: &str) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
        let h = *windows_of(&lnk_target(app_key)?).first().ok_or("aucune fenêtre ouverte pour cette application")?;
        // SAFETY: appels sur une fenêtre existante (au pire déjà fermée : échec sans effet).
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            if !SetForegroundWindow(h).as_bool() {
                return Err("Windows a refusé le premier plan".into());
            }
        }
        Ok(())
    }
    fn close(&self, app_key: &str) -> Result<(), String> {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let ws = windows_of(&lnk_target(app_key)?);
        if ws.is_empty() {
            return Err("aucune fenêtre ouverte pour cette application".into());
        }
        for h in ws {
            // SAFETY: simple message posté ; jamais TerminateProcess.
            unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)) }.map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    async fn run(cmd: &str, secs: u64) -> ShellOutput {
        let t = tempfile::tempdir().unwrap();
        RealShell.run(cmd, t.path(), Duration::from_secs(secs)).await.unwrap()
    }

    #[tokio::test]
    async fn rend_l_unicode() {
        let o = run("Write-Output 'é'", 30).await;
        assert_eq!(o.output.trim(), "é");
        assert_eq!(o.exit_code, Some(0));
    }

    #[tokio::test]
    async fn erreur_en_texte_brut() {
        let o = run(r"Get-Item C:\nope_zz", 30).await;
        assert!(!o.output.contains("CLIXML") && o.output.contains("nope_zz"), "{}", o.output);
    }

    #[tokio::test]
    async fn codes_de_sortie() {
        assert_eq!(run("cmd /c exit 3", 30).await.exit_code, Some(3));
        assert_eq!(run("Write-Output ok", 30).await.exit_code, Some(0));
        let o = run(r"Get-Item C:\nope_zz", 30).await;
        assert!(o.exit_code.is_some_and(|c| c != 0), "{o:?}");
    }

    #[tokio::test]
    async fn delai_depasse() {
        let t0 = Instant::now();
        let o = run("Start-Sleep 5", 1).await;
        assert!(o.timed_out);
        assert!(t0.elapsed() < Duration::from_secs(3));
    }

    /// Écrit un raccourci dans un dossier temporaire (rien d'autre n'est touché).
    fn make_lnk(dir: &Path, target: &str) -> String {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        let p = dir.join("App.lnk").to_string_lossy().into_owned();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(&HSTRING::from(target)).unwrap();
            link.cast::<IPersistFile>().unwrap().Save(&HSTRING::from(p.as_str()), true).unwrap();
        }
        p
    }

    #[test]
    fn lit_la_cible_d_un_raccourci_sans_trouver_de_fenetre() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("absent_zz.exe");
        let lnk = make_lnk(t.path(), &exe.to_string_lossy());
        assert_eq!(lnk_target(&lnk).unwrap().to_string_lossy().to_lowercase(), exe.to_string_lossy().to_lowercase());
        // Aucun processus n'a cette image : ni premier plan ni fermeture n'ont lieu.
        assert!(Win32Windows.focus(&lnk).is_err());
        assert!(Win32Windows.close(&lnk).is_err());
    }

    #[test]
    fn sid_de_l_utilisateur() {
        assert!(user_sid().unwrap().starts_with("S-1-5-"));
    }

    /// Vraie Corbeille (R8 : hors des tests courants).
    #[test]
    #[ignore]
    fn vraie_corbeille_aller_retour() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("oniris_corbeille_zz.txt");
        std::fs::write(&p, "x").unwrap();
        let after = oniris_core::time_util::now_ms() - 2_000;
        ShellRecycler.recycle(&p).unwrap();
        assert!(!p.exists());
        ShellRecycler.restore(&p, after).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "x");
    }

    /// Vraies fenêtres (R8 : hors des tests courants). `ONIRIS_TEST_LNK` : raccourci d'une application ouverte.
    #[test]
    #[ignore]
    fn vraies_fenetres_premier_plan_puis_fermeture() {
        let lnk = std::env::var("ONIRIS_TEST_LNK").expect("ONIRIS_TEST_LNK");
        Win32Windows.focus(&lnk).unwrap();
        Win32Windows.close(&lnk).unwrap();
    }

    #[tokio::test]
    async fn sortie_tronquee() {
        let o = run("Write-Output ('a' * 100000)", 30).await;
        assert_eq!(o.output.len(), MAX_OUTPUT);
    }
}
