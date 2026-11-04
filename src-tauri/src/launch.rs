//! Ouverture réelle de fichiers, liens et processus (jamais appelée par les tests : R8).
use oniris_core::launch::Opener;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[allow(dead_code)] // utilisé par spawn_hidden (tâches suivantes)
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// `ShellExecuteW "open"` sur un fil annexe (jamais le fil principal ni un worker tokio). Seul le code d'échec est journalisé.
fn shell_open(target: std::ffi::OsString) {
    let spawned = std::thread::Builder::new().name("oniris-open".into()).spawn(move || {
        let wide: Vec<u16> = target.encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide` est terminée par un nul et vit pendant l'appel.
        let r = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if (r.0 as isize) <= 32 {
            tracing::warn!("ouverture refusée (code {})", r.0 as isize);
        }
    });
    if let Err(e) = spawned {
        tracing::error!("ouverture : fil impossible : {e}");
    }
}

pub fn open_path(p: &Path) {
    shell_open(p.as_os_str().to_owned());
}

/// À n'appeler qu'avec une URL validée par `oniris_core::launch::safe_url` (ou `safe_vscode_claude_uri`).
pub fn open_url(u: &str) {
    shell_open(u.into());
}

/// Outil console : sans fenêtre, arguments séparés (aucune chaîne shell).
#[allow(dead_code)] // appelé par les tâches suivantes
pub fn spawn_hidden(program: &str, args: &[String], cwd: Option<&Path>) -> std::io::Result<Child> {
    spawn(program, args, cwd, CREATE_NO_WINDOW)
}

/// Application graphique : détachée de notre console et de notre cycle de vie.
pub fn spawn_detached(program: &str, args: &[String], cwd: Option<&Path>) -> std::io::Result<Child> {
    spawn(program, args, cwd, DETACHED_PROCESS)
}

fn spawn(program: &str, args: &[String], cwd: Option<&Path>, flags: u32) -> std::io::Result<Child> {
    let mut c = Command::new(program);
    c.args(args).creation_flags(flags);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    c.spawn()
}

pub struct RealOpener;

impl Opener for RealOpener {
    fn open_path(&self, p: &Path) {
        open_path(p);
    }
    fn open_url(&self, u: &str) {
        open_url(u);
    }
    fn spawn_detached(&self, program: &str, args: &[String], cwd: Option<&Path>) -> std::io::Result<()> {
        spawn_detached(program, args, cwd).map(drop)
    }
}
