/// Rectangle en pixels physiques.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

pub fn pill_rect(monitor: Rect, scale: f64, content_width_logical: f64) -> Rect {
    let w = (content_width_logical.clamp(160.0, 640.0) * scale).round() as u32;
    let h = (48.0 * scale).round() as u32;
    Rect { x: monitor.x + (monitor.w as i32 - w as i32) / 2, y: monitor.y, w, h }
}

pub fn panel_rect(monitor: Rect) -> Rect {
    monitor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_centree_en_haut_a_100_pourcent() {
        let m = Rect { x: 0, y: 0, w: 2560, h: 1440 };
        assert_eq!(pill_rect(m, 1.0, 400.0), Rect { x: 1080, y: 0, w: 400, h: 48 });
    }
    #[test]
    fn pill_a_150_pourcent_sur_ecran_decale() {
        let m = Rect { x: -1920, y: 0, w: 1920, h: 1080 };
        assert_eq!(pill_rect(m, 1.5, 400.0), Rect { x: -1920 + 660, y: 0, w: 600, h: 72 });
    }
    #[test]
    fn pill_largeur_bornee() {
        let m = Rect { x: 0, y: 0, w: 2560, h: 1440 };
        assert_eq!(pill_rect(m, 1.0, 5000.0).w, 640);
        assert_eq!(pill_rect(m, 1.0, 10.0).w, 160);
    }
    #[test]
    fn panel_couvre_l_ecran() {
        let m = Rect { x: -1920, y: 0, w: 1920, h: 1080 };
        assert_eq!(panel_rect(m), m);
    }
}
