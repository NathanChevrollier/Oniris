//! Crochet clavier bas niveau du raccourci global, sur un thread dédié qui possède sa boucle de messages.
//!
//! La callback doit répondre très vite (Windows retire un crochet trop lent) : pas d'appel Tauri, les
//! actions partent dans un canal vers un thread qui les confie au thread principal.

use super::classifier::{Classifier, HotkeyAction, KeyEvent};
use super::HotkeyConfig;
use ::windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LWIN, VK_MENU, VK_RMENU, VK_RWIN,
    VK_SHIFT,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KillTimer, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
    HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_TIMER,
};
use parking_lot::RwLock;
use std::cell::RefCell;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// Marque (dwExtraInfo) de nos propres frappes injectées : notre crochet les ignore. Les entrées injectées par
/// d'autres programmes (logiciels de clavier, manettes émulées) restent prises en compte.
pub const ONIRIS_MARK: usize = 0x4F4E_4952;
/// Touche virtuelle non attribuée : frappée avant le relâchement d'Alt, elle empêche l'activation du menu.
const MENU_MASK_VK: u16 = 0xE8;
const TICK_MS: u32 = 40;
/// Windows retire sans prévenir un crochet qui a tardé (machine chargée, sortie de veille) : on le repose.
const REINSTALL_MS: u32 = 30_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Modifiers {
    alt: bool,
    ctrl: bool,
    shift: bool,
    win: bool,
}

impl Modifiers {
    fn of(c: &HotkeyConfig) -> Self {
        Self {
            alt: c.alt,
            ctrl: c.ctrl,
            shift: c.shift,
            win: c.win,
        }
    }
}

/// État du raccourci vu par le crochet.
#[derive(Debug, Default)]
struct Chord {
    /// Touche principale tenue pendant un raccourci actif.
    held_vk: Option<u16>,
    /// Le raccourci a servi avec Alt ou Win : masquer le menu au prochain relâchement de l'un d'eux.
    mask_pending: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Decision {
    swallow: bool,
    inject_mask: bool,
    key_event: Option<KeyEvent>,
}

fn is_alt_or_win(vk: u16) -> bool {
    [VK_MENU, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|k| k.0 == vk)
}

/// Décision pure pour une frappe : avaler, injecter le masque de menu, signaler un appui/relâchement.
/// `ours` : frappe portant `ONIRIS_MARK`. `mods` : modificateurs tenus au moment de la frappe.
fn decide(
    chord: &mut Chord,
    cfg: &HotkeyConfig,
    vk: u16,
    down: bool,
    ours: bool,
    mods: Modifiers,
    at_ms: u64,
) -> Decision {
    let mut d = Decision::default();
    if ours {
        return d;
    }
    if chord.held_vk == Some(vk) {
        // Répétition automatique avalée ; relâchement (quels que soient les modificateurs) : fin du raccourci.
        d.swallow = true;
        if !down {
            chord.held_vk = None;
            d.key_event = Some(KeyEvent::ChordUp { at_ms });
        }
    } else if !down && chord.mask_pending && is_alt_or_win(vk) {
        // Le modificateur n'est jamais avalé : le masque passe juste avant lui.
        chord.mask_pending = false;
        d.inject_mask = true;
    } else if down && vk == cfg.vk && mods == Modifiers::of(cfg) {
        chord.held_vk = Some(vk);
        chord.mask_pending |= cfg.alt || cfg.win;
        d.swallow = true;
        d.key_event = Some(KeyEvent::ChordDown { at_ms });
    }
    d
}

struct Hook {
    cfg: Arc<RwLock<HotkeyConfig>>,
    tx: Sender<HotkeyAction>,
    classifier: Classifier,
    chord: Chord,
    tick_timer: usize,
}

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

/// Horloge monotone des `KeyEvent`.
fn mono_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn current_modifiers() -> Modifiers {
    Modifiers {
        alt: pressed(VK_MENU),
        ctrl: pressed(VK_CONTROL),
        shift: pressed(VK_SHIFT),
        win: pressed(VK_LWIN) || pressed(VK_RWIN),
    }
}

fn key(vk: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: ONIRIS_MARK,
            },
        },
    }
}

/// Frappe factice glissée avant le relâchement d'Alt/Win : l'application au premier plan n'active pas sa
/// barre de menu (ni le menu Démarrer pour Win).
fn mask_menu() {
    let inputs = [
        key(MENU_MASK_VK, KEYBD_EVENT_FLAGS(0)),
        key(MENU_MASK_VK, KEYEVENTF_KEYUP),
    ];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

impl Hook {
    fn feed(&mut self, ev: KeyEvent) -> Option<HotkeyAction> {
        let action = self.classifier.feed(ev)?;
        let _ = self.tx.send(action);
        Some(action)
    }

    fn on_key(&mut self, vk: u16, down: bool) -> Decision {
        let cfg = *self.cfg.read();
        let d = decide(
            &mut self.chord,
            &cfg,
            vk,
            down,
            false,
            current_modifiers(),
            mono_ms(),
        );
        match d.key_event {
            Some(ev @ KeyEvent::ChordDown { .. }) => {
                // Seuil relu à chaque appui : un changement de réglage s'applique sans redémarrer le crochet.
                self.classifier = Classifier::new(cfg.threshold_ms);
                self.feed(ev);
                self.tick_timer = unsafe { SetTimer(None, 0, TICK_MS, None) };
            }
            Some(ev) => {
                unsafe {
                    let _ = KillTimer(None, self.tick_timer);
                }
                self.feed(ev);
            }
            None => {}
        }
        d
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        if kb.dwExtraInfo != ONIRIS_MARK && (down || msg == WM_KEYUP || msg == WM_SYSKEYUP) {
            // try_borrow_mut : une réentrée laisse passer la frappe au lieu de paniquer dans un callback système.
            let d = HOOK.with(|h| match h.try_borrow_mut() {
                Ok(mut h) => h.as_mut().map(|h| h.on_key(kb.vkCode as u16, down)),
                Err(_) => None,
            });
            if let Some(d) = d {
                // Hors de l'emprunt : SendInput rappelle ce crochet (frappes marquées, donc ignorées).
                if d.inject_mask {
                    mask_menu();
                }
                if d.swallow {
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe fn install() -> Option<HHOOK> {
    let module = GetModuleHandleW(None).ok()?;
    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(module.into()), 0).ok()
}

#[derive(Clone, serde::Serialize)]
struct Ptt {
    phase: &'static str,
    at_ms: u64,
}

fn dispatch(app: &AppHandle, action: HotkeyAction) {
    let ptt = |phase| {
        #[cfg(debug_assertions)]
        eprintln!("Oniris : ptt {phase}");
        app.emit(
            "oniris://ptt",
            Ptt {
                phase,
                at_ms: crate::windows::now_ms(),
            },
        )
    };
    let result = match action {
        HotkeyAction::TogglePanel => crate::windows::toggle_panel(app),
        HotkeyAction::PttStart => ptt("start"),
        HotkeyAction::PttEnd => ptt("end"),
    };
    if let Err(e) = result {
        eprintln!("Oniris : action du raccourci impossible : {e}");
    }
}

/// Installe le crochet du raccourci global ; `cfg` peut être modifié à chaud.
pub fn start_hook(app: AppHandle, cfg: Arc<RwLock<HotkeyConfig>>) {
    let (tx, rx) = channel::<HotkeyAction>();
    let actions = std::thread::Builder::new()
        .name("hotkey-actions".into())
        .spawn(move || {
            for action in rx {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || dispatch(&handle, action));
            }
        });
    let hook = std::thread::Builder::new()
        .name("hotkey-hook".into())
        .spawn(move || unsafe {
            // Le système attend la callback à chaque frappe : le thread doit être servi en priorité.
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            let threshold = cfg.read().threshold_ms;
            HOOK.with(|h| {
                *h.borrow_mut() = Some(Hook {
                    cfg,
                    tx,
                    classifier: Classifier::new(threshold),
                    chord: Chord::default(),
                    tick_timer: 0,
                })
            });
            let Some(mut hook) = install() else {
                eprintln!("Oniris : installation du crochet clavier impossible");
                return;
            };
            let reinstall_timer = SetTimer(None, 0, REINSTALL_MS, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message != WM_TIMER {
                    continue;
                }
                if msg.wParam.0 == reinstall_timer {
                    // Le nouveau crochet est posé avant de retirer l'ancien : aucune frappe n'est perdue.
                    if let Some(fresh) = install() {
                        let _ = UnhookWindowsHookEx(hook);
                        hook = fresh;
                    }
                } else {
                    // Un Tick resté en file après le relâchement est ignoré par le classifieur (état repos).
                    HOOK.with(|h| {
                        if let Some(h) = h.borrow_mut().as_mut() {
                            // Les ticks ne servent qu'à atteindre le seuil : plus de réveils une fois le maintien parti.
                            if h.feed(KeyEvent::Tick { at_ms: mono_ms() }) == Some(HotkeyAction::PttStart) {
                                let _ = KillTimer(None, h.tick_timer);
                            }
                        }
                    });
                }
            }
        });
    if let Err(e) = actions.and(hook) {
        eprintln!("Oniris : démarrage du raccourci impossible : {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: u16 = 0x20;
    const ALT: Modifiers = Modifiers {
        alt: true,
        ctrl: false,
        shift: false,
        win: false,
    };
    const NONE: Modifiers = Modifiers {
        alt: false,
        ctrl: false,
        shift: false,
        win: false,
    };

    fn run(chord: &mut Chord, vk: u16, down: bool, mods: Modifiers) -> Decision {
        decide(chord, &HotkeyConfig::default(), vk, down, false, mods, 0)
    }

    #[test]
    fn appui_du_raccourci_avale() {
        let mut c = Chord::default();
        let d = run(&mut c, SPACE, true, ALT);
        assert!(d.swallow && !d.inject_mask);
        assert_eq!(d.key_event, Some(KeyEvent::ChordDown { at_ms: 0 }));
        // Répétition automatique : avalée, sans nouvel événement.
        assert_eq!(
            run(&mut c, SPACE, true, ALT),
            Decision {
                swallow: true,
                ..Default::default()
            }
        );
        let d = run(&mut c, SPACE, false, ALT);
        assert!(d.swallow);
        assert_eq!(d.key_event, Some(KeyEvent::ChordUp { at_ms: 0 }));
    }

    #[test]
    fn espace_seul_ou_autres_modificateurs_passe() {
        let mut c = Chord::default();
        assert_eq!(run(&mut c, SPACE, true, NONE), Decision::default());
        assert_eq!(run(&mut c, SPACE, false, NONE), Decision::default());
        let ctrl_alt = Modifiers { ctrl: true, ..ALT };
        assert_eq!(run(&mut c, SPACE, true, ctrl_alt), Decision::default());
    }

    #[test]
    fn modificateurs_jamais_avales() {
        let mut c = Chord::default();
        assert!(!run(&mut c, VK_LMENU.0, true, NONE).swallow);
        run(&mut c, SPACE, true, ALT);
        for vk in [VK_LMENU.0, VK_RMENU.0, VK_CONTROL.0, VK_SHIFT.0, VK_LWIN.0] {
            assert!(!run(&mut c, vk, true, ALT).swallow);
            assert!(!run(&mut c, vk, false, ALT).swallow);
        }
    }

    #[test]
    fn masque_une_seule_fois_au_relachement_d_alt() {
        let mut c = Chord::default();
        run(&mut c, SPACE, true, ALT);
        run(&mut c, SPACE, false, ALT);
        assert_eq!(
            run(&mut c, VK_LMENU.0, false, ALT),
            Decision {
                inject_mask: true,
                ..Default::default()
            }
        );
        assert_eq!(run(&mut c, VK_LMENU.0, false, NONE), Decision::default());
    }

    #[test]
    fn pas_de_masque_sans_raccourci() {
        let mut c = Chord::default();
        run(&mut c, VK_LMENU.0, true, NONE);
        assert_eq!(run(&mut c, VK_LMENU.0, false, ALT), Decision::default());
    }

    #[test]
    fn frappes_marquees_ignorees() {
        let mut c = Chord::default();
        let cfg = HotkeyConfig::default();
        assert_eq!(
            decide(&mut c, &cfg, SPACE, true, true, ALT, 0),
            Decision::default()
        );
        assert_eq!(c.held_vk, None);
        run(&mut c, SPACE, true, ALT);
        // Notre masque injecté ne termine pas le raccourci et ne consomme pas le masque en attente.
        assert_eq!(
            decide(&mut c, &cfg, MENU_MASK_VK, false, true, ALT, 0),
            Decision::default()
        );
        assert_eq!(
            decide(&mut c, &cfg, VK_LMENU.0, false, true, ALT, 0),
            Decision::default()
        );
        assert!(c.held_vk.is_some() && c.mask_pending);
    }

    #[test]
    fn alt_relache_avant_espace_termine_le_maintien() {
        let mut c = Chord::default();
        run(&mut c, SPACE, true, ALT);
        assert!(run(&mut c, VK_LMENU.0, false, ALT).inject_mask);
        let d = run(&mut c, SPACE, false, NONE);
        assert!(d.swallow);
        assert_eq!(d.key_event, Some(KeyEvent::ChordUp { at_ms: 0 }));
        assert_eq!(c.held_vk, None);
        // Pas d'état « tenu » bloqué : Espace seul repasse ensuite normalement.
        assert_eq!(run(&mut c, SPACE, true, NONE), Decision::default());
    }
}
