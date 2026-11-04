//! Codes météo WMO d'Open-Meteo vers un libellé français et un nom d'icône Lucide.

pub fn label_icon(code: u8) -> (&'static str, &'static str) {
    match code {
        0 => ("Ciel dégagé", "Sun"),
        1 => ("Plutôt dégagé", "CloudSun"),
        2 => ("Partiellement nuageux", "CloudSun"),
        3 => ("Couvert", "Cloud"),
        45 => ("Brouillard", "CloudFog"),
        48 => ("Brouillard givrant", "CloudFog"),
        51 => ("Bruine légère", "CloudDrizzle"),
        53 => ("Bruine", "CloudDrizzle"),
        55 => ("Bruine dense", "CloudDrizzle"),
        56 => ("Bruine verglaçante légère", "CloudDrizzle"),
        57 => ("Bruine verglaçante dense", "CloudDrizzle"),
        61 => ("Pluie faible", "CloudRain"),
        63 => ("Pluie modérée", "CloudRain"),
        65 => ("Pluie forte", "CloudRain"),
        66 => ("Pluie verglaçante légère", "CloudRain"),
        67 => ("Pluie verglaçante forte", "CloudRain"),
        71 => ("Neige faible", "CloudSnow"),
        73 => ("Neige modérée", "CloudSnow"),
        75 => ("Neige forte", "CloudSnow"),
        77 => ("Grains de neige", "CloudSnow"),
        80 => ("Averses faibles", "CloudRain"),
        81 => ("Averses modérées", "CloudRain"),
        82 => ("Averses violentes", "CloudRain"),
        85 => ("Averses de neige faibles", "CloudSnow"),
        86 => ("Averses de neige fortes", "CloudSnow"),
        95 => ("Orage", "CloudLightning"),
        96 => ("Orage avec grêle", "CloudLightning"),
        99 => ("Orage avec forte grêle", "CloudLightning"),
        _ => ("Conditions inconnues", "Cloud"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libelles_connus_et_inconnus() {
        assert_eq!(label_icon(61), ("Pluie faible", "CloudRain"));
        assert_eq!(label_icon(0), ("Ciel dégagé", "Sun"));
        assert_eq!(label_icon(95).1, "CloudLightning");
        assert_eq!(label_icon(42), ("Conditions inconnues", "Cloud"));
        for c in [0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82, 85, 86, 95, 96, 99] {
            assert_ne!(label_icon(c).0, "Conditions inconnues", "code {c}");
        }
    }
}
