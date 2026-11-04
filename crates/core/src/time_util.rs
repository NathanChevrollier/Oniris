//! Bornes de jour locales. Jamais « + 24 h » : un jour de changement d'heure dure 23 h ou 25 h.
use chrono::{Duration, Local, NaiveDate, TimeZone};

/// Minuit local de `d` ; dans un trou de changement d'heure, le premier instant valide.
fn midnight<Tz: TimeZone>(tz: &Tz, d: NaiveDate) -> Option<i64> {
    let mut n = d.and_hms_opt(0, 0, 0)?;
    for _ in 0..3 {
        if let Some(t) = tz.from_local_datetime(&n).earliest() {
            return Some(t.timestamp_millis());
        }
        n += Duration::hours(1);
    }
    None
}

/// [minuit du jour de `ms`, minuit suivant[ dans le fuseau `tz`.
pub fn day_bounds_in<Tz: TimeZone>(tz: &Tz, ms: i64) -> (i64, i64) {
    let Some(t) = tz.timestamp_millis_opt(ms).earliest() else { return (ms, ms) };
    let d = t.date_naive();
    let start = midnight(tz, d).unwrap_or(ms);
    let end = d.succ_opt().and_then(|n| midnight(tz, n)).unwrap_or(start);
    (start, end)
}

pub fn next_midnight_in<Tz: TimeZone>(tz: &Tz, ms: i64) -> i64 { day_bounds_in(tz, ms).1 }

/// Bornes du jour local de `ms` (début inclus, fin exclue).
pub fn local_day_bounds(ms: i64) -> (i64, i64) { day_bounds_in(&Local, ms) }

/// Prochain minuit local strictement après `ms`.
pub fn next_local_midnight(ms: i64) -> i64 { next_midnight_in(&Local, ms) }

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Paris;

    fn ms(y: i32, m: u32, d: u32, h: u32) -> i64 {
        Paris.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap().timestamp_millis()
    }
    const H: i64 = 3_600_000;

    #[test]
    fn jours_de_changement_d_heure_font_23_h_et_25_h() {
        let (a, b) = day_bounds_in(&Paris, ms(2026, 3, 29, 12));
        assert_eq!((b - a) / H, 23);
        let (a, b) = day_bounds_in(&Paris, ms(2026, 10, 25, 12));
        assert_eq!((b - a) / H, 25);
        let (a, b) = day_bounds_in(&Paris, ms(2026, 11, 4, 12));
        assert_eq!((a, b - a), (ms(2026, 11, 4, 0), 24 * H));
        assert_eq!(next_midnight_in(&Paris, ms(2026, 3, 28, 23)), ms(2026, 3, 29, 0));
    }
}
