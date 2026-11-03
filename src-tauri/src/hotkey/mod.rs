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

/// "Space", "A"-"Z", "F1"-"F12" vers un code VK ; `None` si inconnue.
pub fn key_to_vk(key: &str) -> Option<u16> {
    let b = key.as_bytes();
    match key {
        "Space" => Some(0x20),
        _ if b.len() == 1 && b[0].is_ascii_uppercase() => Some(b[0] as u16),
        _ => match key.strip_prefix('F')?.parse::<u16>() {
            // exclut "F01" et "F+1", acceptés par parse
            Ok(n @ 1..=12) if key[1..].bytes().all(|c| c.is_ascii_digit()) && !key[1..].starts_with('0') => Some(0x70 + n - 1),
            _ => None,
        },
    }
}

impl HotkeyConfig {
    pub fn from_setting(s: &oniris_core::HotkeySetting, threshold_ms: u64) -> Self {
        let vk = key_to_vk(&s.key).unwrap_or_else(|| {
            eprintln!("oniris: touche de raccourci inconnue {:?}, Espace conservée", s.key);
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
