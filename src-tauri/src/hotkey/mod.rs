pub mod classifier;
mod hook;

pub use hook::start_hook;
/// Frappe marquée `ONIRIS_MARK` (ignorée par notre crochet), pour nos injections.
pub(crate) use hook::key as marked_key;
pub use oniris_core::key_to_vk;

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

impl HotkeyConfig {
    pub fn from_setting(s: &oniris_core::HotkeySetting, threshold_ms: u64) -> Self {
        let vk = key_to_vk(&s.key).unwrap_or_else(|| {
            tracing::warn!("touche de raccourci inconnue {:?}, Espace conservée", s.key);
            0x20
        });
        Self { alt: s.alt, ctrl: s.ctrl, shift: s.shift, win: s.win, vk, threshold_ms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn correspondance_des_touches() {
        assert_eq!(key_to_vk("Space"), Some(0x20));
        assert_eq!(key_to_vk("A"), Some(0x41));
        assert_eq!(key_to_vk("Z"), Some(0x5A));
        assert_eq!(key_to_vk("F1"), Some(0x70));
        assert_eq!(key_to_vk("F12"), Some(0x7B));
        for k in ["", "a", "F0", "F13", "F01", "AB", "Enter", "F+1"] {
            assert_eq!(key_to_vk(k), None, "{k}");
        }
    }
    #[test]
    fn config_depuis_les_reglages() {
        let s = oniris_core::HotkeySetting { alt: false, ctrl: true, shift: true, win: false, key: "F5".into() };
        let c = HotkeyConfig::from_setting(&s, 300);
        assert_eq!((c.alt, c.ctrl, c.shift, c.win, c.vk, c.threshold_ms), (false, true, true, false, 0x74, 300));
        let bad = oniris_core::HotkeySetting { key: "??".into(), ..s };
        assert_eq!(HotkeyConfig::from_setting(&bad, 250).vk, 0x20);
    }
}
