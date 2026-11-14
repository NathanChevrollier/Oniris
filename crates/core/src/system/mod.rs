//! État du PC : forme du instantané et calcul pur du GPU. L'échantillonnage (sysinfo, PDH) vit dans `src-tauri`.
pub mod tools;

use serde::Serialize;
use std::collections::HashMap;

pub const NOT_INSTALLED: &str = "non installée";

/// État d'Ollama pour la carte « Ce PC » : un processus `ollama.exe` tourne, sinon installé mais arrêté, sinon absent.
pub fn local_ai_label(running: bool, installed: bool) -> &'static str {
    if running { "en marche" } else if installed { "arrêtée" } else { NOT_INSTALLED }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemStatus {
    pub cpu_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpu_pct: Option<f32>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub oniris_mb: u64,
    pub local_ai: String,
    pub voice: String,
}

/// Identifiant d'adaptateur : le texte entre `luid_` et `_phys`.
fn luid(name: &str) -> Option<&str> {
    let rest = name.split_once("luid_")?.1;
    Some(rest.split_once("_phys").map_or(rest, |(l, _)| l))
}

/// Utilisation GPU comme le Gestionnaire des tâches : somme par (luid, type de moteur), puis maximum. Bornée à 0..=100.
pub fn gpu_from_counters(engine_utils: &[(String, f64)]) -> f32 {
    let mut sums: HashMap<(&str, &str), f64> = HashMap::new();
    for (name, v) in engine_utils {
        let (Some(luid), Some(ty)) = (luid(name), name.rsplit_once("engtype_").map(|(_, t)| t)) else { continue };
        *sums.entry((luid, ty)).or_default() += v;
    }
    sums.values().fold(0.0_f64, |a, &b| a.max(b)).clamp(0.0, 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: &str, v: f64) -> (String, f64) { (n.into(), v) }

    #[test]
    fn gpu_somme_par_type_de_moteur_puis_maximum() {
        let l = "luid_0x00000000_0x0000D1A4_phys_0";
        let u = vec![
            s(&format!("pid_1234_{l}_eng_0_engtype_3D"), 20.0),
            s(&format!("pid_5678_{l}_eng_0_engtype_3D"), 15.5),
            s(&format!("pid_5678_{l}_eng_1_engtype_Copy"), 30.0),
            s(&format!("pid_99_{l}_eng_2_engtype_VideoDecode"), 5.0),
        ];
        assert_eq!(gpu_from_counters(&u), 35.5); // 3D (20 + 15,5) > Copy (30)
    }

    #[test]
    fn gpu_separe_les_adaptateurs_et_borne() {
        let a = "luid_0x00000000_0x0000AAAA_phys_0";
        let b = "luid_0x00000000_0x0000BBBB_phys_0";
        let u = vec![s(&format!("pid_1_{a}_eng_0_engtype_3D"), 60.0), s(&format!("pid_2_{b}_eng_0_engtype_3D"), 70.0), s(&format!("pid_3_{b}_eng_0_engtype_3D"), 70.0)];
        assert_eq!(gpu_from_counters(&u), 100.0, "70 + 70 borné à 100, sans mélanger avec l'autre luid");
        assert_eq!(gpu_from_counters(&[]), 0.0);
        assert_eq!(gpu_from_counters(&[s("n_importe_quoi", 50.0)]), 0.0);
    }

    #[test]
    fn statut_serialise_gpu_null() {
        let st = SystemStatus { cpu_pct: 1.0, ram_used_mb: 2, ram_total_mb: 4, gpu_pct: None, vram_used_mb: None, vram_total_mb: None, oniris_mb: 3, local_ai: NOT_INSTALLED.into(), voice: NOT_INSTALLED.into() };
        let v = serde_json::to_value(&st).unwrap();
        assert!(v["gpu_pct"].is_null() && v["vram_used_mb"].is_null() && v["vram_total_mb"].is_null());
        assert_eq!(v["local_ai"], "non installée");
    }

    #[test]
    fn etat_de_l_ia_locale() {
        assert_eq!(local_ai_label(true, true), "en marche");
        assert_eq!(local_ai_label(true, false), "en marche");
        assert_eq!(local_ai_label(false, true), "arrêtée");
        assert_eq!(local_ai_label(false, false), "non installée");
    }
}
