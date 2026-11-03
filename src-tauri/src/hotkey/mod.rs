pub mod classifier;
mod hook;

pub use hook::start_hook;

/// Raccourci global : modificateurs exacts + touche principale (code VK) et seuil du maintien.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyConfig {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u16,
    pub threshold_ms: u64,
}

impl Default for HotkeyConfig {
    /// Alt+Espace, maintien à partir de 250 ms.
    fn default() -> Self {
        Self {
            alt: true,
            ctrl: false,
            shift: false,
            win: false,
            vk: 0x20,
            threshold_ms: 250,
        }
    }
}
