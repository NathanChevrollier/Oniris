//! Ce qui peut entrer dans l'historique : formats d'exclusion posés par l'application source et taille du texte.

/// Formats Windows lus sur le presse-papiers. `Some(v)` : format présent, valeur DWORD `v`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClipFlags {
    /// `ExcludeClipboardContentFromMonitorProcessing` présent (sa seule présence suffit).
    pub exclude_monitor: bool,
    /// `CanIncludeInClipboardHistory`.
    pub history_allowed: Option<u32>,
    /// `CanUploadToCloudClipboard`.
    pub cloud_allowed: Option<u32>,
}

/// Faux pour une copie de gestionnaire de mots de passe (ou toute source qui refuse l'historique).
pub fn should_record(flags: &ClipFlags) -> bool {
    !flags.exclude_monitor && flags.history_allowed != Some(0) && flags.cloud_allowed != Some(0)
}

pub const MAX_TEXT_BYTES: usize = 65_536;

/// Texte coupé à `MAX_TEXT_BYTES` octets au plus, sur une frontière de caractère ; `true` s'il a été coupé.
pub fn truncate_text(s: &str) -> (String, bool) {
    if s.len() <= MAX_TEXT_BYTES { return (s.to_string(), false) }
    let end = (0..=MAX_TEXT_BYTES).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
    (s[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_d_exclusion() {
        assert!(should_record(&ClipFlags::default()), "aucun format : enregistré");
        assert!(!should_record(&ClipFlags { exclude_monitor: true, ..Default::default() }));
        assert!(!should_record(&ClipFlags { history_allowed: Some(0), ..Default::default() }));
        assert!(!should_record(&ClipFlags { cloud_allowed: Some(0), ..Default::default() }));
        assert!(should_record(&ClipFlags { history_allowed: Some(1), cloud_allowed: Some(1), exclude_monitor: false }));
        assert!(!should_record(&ClipFlags { history_allowed: Some(1), cloud_allowed: Some(0), exclude_monitor: false }));
    }

    #[test]
    fn texte_de_10_mo_tronque_sans_couper_un_caractere() {
        let big = "é".repeat(5 * 1024 * 1024 + 1); // 2 octets chacun : 10 Mo et des poussières, frontière impaire
        let (t, cut) = truncate_text(&format!("a{big}"));
        assert!(cut);
        assert!(t.len() <= MAX_TEXT_BYTES && t.len() >= MAX_TEXT_BYTES - 3);
        assert!(t.ends_with('é'));
        let (t, cut) = truncate_text("court");
        assert_eq!((t.as_str(), cut), ("court", false));
        let exact = "x".repeat(MAX_TEXT_BYTES);
        assert_eq!(truncate_text(&exact), (exact.clone(), false));
    }
}
