//! Dates et heures écrites en français ("demain à 10h appeler la banque").
//! `parse_when` est l'API publique (fuseau local) ; `parse_when_tz` est générique
//! pour que les tests ne dépendent pas du fuseau de la machine (ils utilisent Europe/Paris).

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Weekday};

pub const DEFAULT_REMINDER_HOUR: u32 = 9;

#[derive(Debug, Clone, PartialEq)]
pub struct WhenTz<Tz: TimeZone> {
    pub start: DateTime<Tz>,
    pub end: Option<DateTime<Tz>>,
    pub all_day: bool,
    /// Texte privé de l'expression temporelle, nettoyé.
    pub rest: String,
}

pub type When = WhenTz<Local>;

pub fn parse_when(text: &str, now: DateTime<Local>) -> Option<When> {
    parse_when_tz(text, now)
}

/// Pour un jour entier, `start` à l'heure de rappel par défaut (9 h) ; sinon `start`.
pub fn at_default_hour(w: &When) -> DateTime<Local> {
    if !w.all_day {
        return w.start;
    }
    at(&Local, w.start.date_naive(), (DEFAULT_REMINDER_HOUR, 0)).unwrap_or(w.start)
}

type Hm = (u32, u32);

#[derive(Default)]
struct Found {
    date: Option<NaiveDate>,
    rel: Option<Duration>,
    time: Option<Hm>,
    end: Option<Hm>,
    default_time: Option<Hm>,
    /// "ce soir" : jamais un instant passé.
    tonight: bool,
    /// "minuit" = 00:00 du lendemain.
    midnight: bool,
    /// Date écrite mais impossible ("31 février") : tout l'analyse échoue.
    invalid: bool,
}

const LINKS: [&str; 8] = ["a", "le", "de", "pour", "rappel", "rappelle", "rappelle-moi", "moi"];
const MONTHS: [&str; 12] = [
    "janvier", "fevrier", "mars", "avril", "mai", "juin", "juillet", "aout", "septembre",
    "octobre", "novembre", "decembre",
];

pub(crate) fn norm(s: &str) -> String {
    let s: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' | 'ä' | 'á' => 'a',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            '’' => '\'',
            c => c,
        })
        .collect();
    s.replace('œ', "oe").trim().trim_end_matches([',', '.', ';', '!', '?']).trim_end().to_string()
}

fn num(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn at<Tz: TimeZone>(tz: &Tz, d: NaiveDate, (h, m): Hm) -> Option<DateTime<Tz>> {
    let mut n = d.and_hms_opt(h, m, 0)?;
    // Trou de changement d'heure : on avance jusqu'au prochain instant valide ; ambigu : le plus tôt.
    for _ in 0..3 {
        if let Some(t) = tz.from_local_datetime(&n).earliest() {
            return Some(t);
        }
        n += Duration::hours(1);
    }
    None
}

/// "10h", "10h30", "10:30" (un seul mot).
fn parse_hm(s: &str) -> Option<Hm> {
    let (h, m) = s.split_once([':', 'h'])?;
    let m = if m.is_empty() { 0 } else if m.len() == 2 { num(m)? } else { return None };
    Some((num(h).filter(|h| *h < 24)?, Some(m).filter(|m| *m < 60)?))
}

/// Heure sur un ou plusieurs mots ("10h", "10 h 30"). Retourne (heure, mots consommés).
fn parse_time(w: &[String], i: usize) -> Option<(Hm, usize)> {
    if let Some(t) = parse_hm(&w[i]) {
        return Some((t, 1));
    }
    let h = num(&w[i]).filter(|h| *h < 24)?;
    if !matches!(w.get(i + 1).map(String::as_str), Some("h" | "heure" | "heures")) {
        return None;
    }
    match w.get(i + 2).and_then(|m| num(m)).filter(|m| *m < 60) {
        Some(m) => Some(((h, m), 3)),
        None => Some(((h, 0), 2)),
    }
}

fn time_form(w: &[String], i: usize, f: &mut Found) -> usize {
    if let Some((a, b)) = w[i].split_once('-') {
        if let (Some(a), Some(b)) = (parse_hm(a), parse_hm(b)) {
            (f.time, f.end) = (Some(a), Some(b));
            return 1;
        }
    }
    let Some((t, n)) = parse_time(w, i) else { return 0 };
    f.time = Some(t);
    let k = i + n;
    if matches!(w.get(k).map(String::as_str), Some("a" | "-")) && k + 1 < w.len() {
        if let Some((e, m)) = parse_time(w, k + 1) {
            f.end = Some(e);
            return n + 1 + m;
        }
    }
    n
}

/// "dans 40 minutes", "dans 1h30", "dans 2 h", "dans 1 heure 30", "dans 3 jours", "dans 2 semaines".
fn relative(w: &[String], i: usize, today: NaiveDate, f: &mut Found) -> usize {
    if w[i] != "dans" {
        return 0;
    }
    let Some(a) = w.get(i + 1) else { return 0 };
    if let Some((h, m)) = a.split_once('h') {
        if let (Some(h), Some(m)) = (num(h), if m.is_empty() { Some(0) } else { num(m).filter(|m| *m < 60) }) {
            f.rel = Some(Duration::minutes(i64::from(h * 60 + m)));
            return 2;
        }
    }
    let (Some(n), Some(unit)) = (num(a), w.get(i + 2)) else { return 0 };
    let n = i64::from(n);
    if unit.starts_with("min") {
        f.rel = Some(Duration::minutes(n));
    } else if unit == "h" || unit.starts_with("heure") {
        let m = w.get(i + 3).and_then(|m| num(m)).filter(|m| *m < 60);
        f.rel = Some(Duration::minutes(n * 60 + m.map_or(0, i64::from)));
        return 3 + usize::from(m.is_some());
    } else if unit.starts_with("jour") {
        f.date = Some(today + Duration::days(n));
    } else if unit.starts_with("semaine") {
        f.date = Some(today + Duration::days(7 * n));
    } else {
        return 0;
    }
    3
}

fn day_word(w: &[String], i: usize, today: NaiveDate, f: &mut Found) -> usize {
    let wd = match w[i].as_str() {
        "aujourd'hui" => return set_date(f, today, 1),
        "demain" => return set_date(f, today + Duration::days(1), 1),
        "apres-demain" => return set_date(f, today + Duration::days(2), 1),
        "ce" if w.get(i + 1).is_some_and(|s| s == "soir") => {
            (f.default_time, f.tonight) = (Some((19, 0)), true);
            return set_date(f, today, 2);
        }
        "matin" => return set_part(f, (9, 0)),
        "midi" => return set_part(f, (12, 0)),
        "apres-midi" => return set_part(f, (15, 0)),
        "soir" => return set_part(f, (19, 0)),
        "nuit" => return set_part(f, (22, 0)),
        "minuit" => {
            f.midnight = true;
            return set_part(f, (0, 0));
        }
        "lundi" => Weekday::Mon,
        "mardi" => Weekday::Tue,
        "mercredi" => Weekday::Wed,
        "jeudi" => Weekday::Thu,
        "vendredi" => Weekday::Fri,
        "samedi" => Weekday::Sat,
        "dimanche" => Weekday::Sun,
        _ => return 0,
    };
    // Toujours strictement après aujourd'hui.
    let d = (wd.num_days_from_monday() + 7 - today.weekday().num_days_from_monday() - 1) % 7 + 1;
    let next = w.get(i + 1).is_some_and(|s| s == "prochain");
    set_date(f, today + Duration::days(i64::from(d)), 1 + usize::from(next))
}

fn set_part(f: &mut Found, t: Hm) -> usize {
    f.default_time = Some(t);
    1
}

fn set_date(f: &mut Found, d: NaiveDate, n: usize) -> usize {
    f.date = Some(d);
    n
}

/// Prochaine occurrence (jour même inclus) : avec mois, année suivante si passé ; sans mois, prochain mois ayant ce jour.
fn next_date(today: NaiveDate, day: u32, month: Option<u32>) -> Option<NaiveDate> {
    (0..60u32)
        .filter_map(|k| match month {
            Some(m) => NaiveDate::from_ymd_opt(today.year() + k as i32, m, day),
            None => {
                let mi = today.month0() + k;
                NaiveDate::from_ymd_opt(today.year() + (mi / 12) as i32, mi % 12 + 1, day)
            }
        })
        .find(|d| *d >= today)
}

fn date_form(w: &[String], i: usize, today: NaiveDate, f: &mut Found) -> usize {
    let le = usize::from(w[i] == "le");
    let Some(s) = w.get(i + le) else { return 0 };
    let dnum = |s: &str| num(s.strip_suffix("er").unwrap_or(s));
    let (day, month, n) = if let Some((d, m)) = s.split_once('/') {
        // "1/2 litre" n'est pas une date : il faut "le" ou une heure derrière.
        let (Some(d), Some(m)) = (num(d).filter(|d| (1..=31).contains(d)), num(m).filter(|m| (1..=12).contains(m)))
        else {
            return 0;
        };
        if le == 0 && (i + 1 >= w.len() || parse_time(w, i + 1).is_none()) {
            return 0;
        }
        (Some(d), Some(m), 1)
    } else if let Some(m) = w.get(i + le + 1).and_then(|m| MONTHS.iter().position(|x| x == m)) {
        (dnum(s), Some(m as u32 + 1), 2)
    } else if le == 1 && !matches!(w.get(i + 2).map(String::as_str), Some("h" | "heure" | "heures")) {
        (dnum(s), None, 1)
    } else {
        return 0;
    };
    let Some(day) = day else { return 0 };
    match next_date(today, day, month) {
        Some(d) => set_date(f, d, le + n),
        None => {
            f.invalid = month.is_some();
            if month.is_some() { le + n } else { 0 }
        }
    }
}

pub fn parse_when_tz<Tz: TimeZone>(text: &str, now: DateTime<Tz>) -> Option<WhenTz<Tz>> {
    let orig: Vec<&str> = text.split_whitespace().collect();
    let w: Vec<String> = orig.iter().map(|s| norm(s)).collect();
    let tz = now.timezone();
    let today = now.date_naive();
    let mut f = Found::default();
    let mut used = vec![false; w.len()];
    let mut i = 0;
    while i < w.len() {
        let mut n = relative(&w, i, today, &mut f);
        if n == 0 {
            n = day_word(&w, i, today, &mut f);
        }
        if n == 0 {
            n = date_form(&w, i, today, &mut f);
        }
        if n == 0 {
            n = time_form(&w, i, &mut f);
        }
        used[i..i + n].fill(true);
        i += n.max(1);
    }
    if f.invalid || (f.date.is_none() && f.rel.is_none() && f.time.is_none() && f.default_time.is_none()) {
        return None;
    }
    // Mots de liaison juste AVANT une expression retirée, ou tout au début du texte
    // (en cascade : "rappelle-moi à 9h") ; jamais après, pour ne pas manger le titre.
    let mut strip = vec![false; w.len()];
    while let Some(k) = (0..w.len()).find(|&k| {
        let link = LINKS.contains(&w[k].as_str()) || (w[k] == "moi" && k > 0 && w[k - 1] == "rappelle");
        !used[k] && link && (used.get(k + 1) == Some(&true) || strip[..k].iter().all(|s| *s))
    }) {
        (used[k], strip[k]) = (true, true);
    }
    let rest = orig.iter().zip(&used).filter(|(_, u)| !**u).map(|(s, _)| *s).collect::<Vec<_>>().join(" ");

    let (start, end, all_day) = if let (Some(d), None) = (f.rel, f.date) {
        (now.clone() + d, None, false)
    } else {
        let time = f.time.or(f.default_time);
        let date = match f.date {
            Some(d) if f.midnight => d + Duration::days(1),
            Some(d) => d,
            None if at(&tz, today, time?)? > now => today,
            None => today + Duration::days(1),
        };
        match time {
            None => (at(&tz, date, (0, 0))?, None, true),
            Some(t) => {
                let s = at(&tz, date, t)?;
                if f.tonight && s <= now {
                    if f.time.is_some() {
                        return None;
                    }
                    return Some(WhenTz { start: now.clone() + Duration::hours(1), end: None, all_day: false, rest });
                }
                let e = match f.end {
                    Some(e) => {
                        let e1 = at(&tz, date, e)?;
                        Some(if e1 <= s { at(&tz, date + Duration::days(1), e)? } else { e1 })
                    }
                    None => None,
                };
                (s, e, false)
            }
        }
    };
    Some(WhenTz { start, end, all_day, rest })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;
    use chrono_tz::Europe::Paris;

    type P = DateTime<chrono_tz::Tz>;

    fn p(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> P {
        Paris.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }
    /// mardi 2026-11-03 14:20
    fn now() -> P {
        p(2026, 11, 3, 14, 20)
    }
    fn parse(t: &str) -> Option<WhenTz<chrono_tz::Tz>> {
        parse_when_tz(t, now())
    }
    fn day(y: i32, mo: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap()
    }

    #[test]
    fn relative_minutes() {
        let w = parse("dans 40 minutes appeler maman").unwrap();
        assert_eq!(w.start, p(2026, 11, 3, 15, 0));
        assert!(!w.all_day);
        assert_eq!(w.rest, "appeler maman");
    }

    #[test]
    fn tomorrow_at_hour() {
        let w = parse("demain à 10h appeler la banque").unwrap();
        assert_eq!(w.start, p(2026, 11, 4, 10, 0));
        assert_eq!(w.rest, "appeler la banque");
    }

    #[test]
    fn past_hour_goes_to_tomorrow() {
        let w = parse("rappel à 9h").unwrap();
        assert_eq!(w.start, p(2026, 11, 4, 9, 0));
        assert_eq!(w.rest, "");
    }

    #[test]
    fn weekday_is_all_day_and_never_today() {
        let w = parse("lundi réunion").unwrap();
        assert_eq!(w.start.date_naive(), day(2026, 11, 9));
        assert!(w.all_day);
        assert_eq!(w.rest, "réunion");
        assert_eq!(parse("mardi prochain x").unwrap().start.date_naive(), day(2026, 11, 10));
    }

    #[test]
    fn date_with_range() {
        let w = parse("le 12 novembre de 14h à 15h30 dentiste").unwrap();
        assert_eq!(w.start, p(2026, 11, 12, 14, 0));
        assert_eq!(w.end, Some(p(2026, 11, 12, 15, 30)));
        assert_eq!(w.rest, "dentiste");
    }

    #[test]
    fn dashed_range() {
        let w = parse("vendredi 18h-19h sport").unwrap();
        assert_eq!(w.start, p(2026, 11, 6, 18, 0));
        assert_eq!(w.end, Some(p(2026, 11, 6, 19, 0)));
        assert_eq!(w.rest, "sport");
    }

    #[test]
    fn dst_tomorrow_is_local_ten() {
        let w = parse_when_tz("demain à 10h", p(2026, 3, 28, 20, 0)).unwrap();
        assert_eq!(w.start, p(2026, 3, 29, 10, 0));
        assert_eq!(w.start.offset().to_string(), "CEST");
    }

    #[test]
    fn dst_gap_moves_forward() {
        let w = parse_when_tz("demain à 2h30", p(2026, 3, 28, 20, 0)).unwrap();
        assert_eq!(w.start, p(2026, 3, 29, 3, 30));
    }

    #[test]
    fn other_forms() {
        assert_eq!(parse("ce soir").unwrap().start, p(2026, 11, 3, 19, 0));
        assert_eq!(parse("après-demain à 10 h 30 x").unwrap().start, p(2026, 11, 5, 10, 30));
        assert_eq!(parse("aujourd'hui 18:15").unwrap().start, p(2026, 11, 3, 18, 15));
        assert_eq!(parse("dans 2 heures").unwrap().start, p(2026, 11, 3, 16, 20));
        assert_eq!(parse("dans 3 jours").unwrap().start.date_naive(), day(2026, 11, 6));
        assert_eq!(parse("le 12 payer").unwrap().start.date_naive(), day(2026, 11, 12));
        assert_eq!(parse("le 2 payer").unwrap().start.date_naive(), day(2026, 12, 2));
        assert_eq!(parse("le 12/11").unwrap().start.date_naive(), day(2026, 11, 12));
        assert_eq!(parse("le 2/11").unwrap().start.date_naive(), day(2027, 11, 2));
        assert_eq!(parse("Rappelle-moi de DEMAIN à 8H sortir").unwrap().rest, "sortir");
    }

    #[test]
    fn dans_durations_not_clock_time() {
        let w = parse("dans 1h30 appeler").unwrap();
        assert_eq!((w.start, w.rest.as_str()), (p(2026, 11, 3, 15, 50), "appeler"));
        assert_eq!(parse("dans 2h").unwrap().start, p(2026, 11, 3, 16, 20));
        let w = parse("dans 1 h 30").unwrap();
        assert_eq!((w.start, w.rest.as_str()), (p(2026, 11, 3, 15, 50), ""));
        assert_eq!(parse("dans 1 heure 30").unwrap().start, p(2026, 11, 3, 15, 50));
        let w = parse("dans 2 semaines").unwrap();
        assert_eq!(w.start.date_naive(), day(2026, 11, 17));
        assert!(w.all_day);
    }

    #[test]
    fn link_words_only_before_or_at_start() {
        assert_eq!(parse("demain le dentiste").unwrap().rest, "le dentiste");
        assert_eq!(parse("appeler demain le dentiste").unwrap().rest, "appeler le dentiste");
        assert_eq!(parse("rappelle-moi demain d'appeler la banque").unwrap().rest, "d'appeler la banque");
        assert_eq!(parse("il a rendez-vous demain").unwrap().rest, "il a rendez-vous");
        assert_eq!(parse("rappelle moi demain appeler").unwrap().rest, "appeler");
    }

    #[test]
    fn parts_of_day() {
        let w = parse("demain soir").unwrap();
        assert_eq!((w.start, w.rest.as_str(), w.all_day), (p(2026, 11, 4, 19, 0), "", false));
        assert_eq!(parse("demain matin").unwrap().start, p(2026, 11, 4, 9, 0));
        assert_eq!(parse("à midi").unwrap().start, p(2026, 11, 4, 12, 0));
        assert_eq!(parse("lundi après-midi").unwrap().start, p(2026, 11, 9, 15, 0));
        assert_eq!(parse("minuit").unwrap().start, p(2026, 11, 4, 0, 0));
        assert_eq!(parse("demain minuit").unwrap().start, p(2026, 11, 5, 0, 0));
        assert_eq!(parse("nuit").unwrap().start, p(2026, 11, 3, 22, 0));
    }

    #[test]
    fn tonight_never_in_the_past() {
        let late = p(2026, 11, 3, 21, 10);
        assert_eq!(parse_when_tz("ce soir", late).unwrap().start, p(2026, 11, 3, 22, 10));
        assert_eq!(parse_when_tz("ce soir 20h", late), None);
        assert_eq!(parse("ce soir 20h").unwrap().start, p(2026, 11, 3, 20, 0));
    }

    #[test]
    fn first_of_month_and_strict_dates() {
        let w = parse("le 1er décembre").unwrap();
        assert_eq!(w.start.date_naive(), day(2026, 12, 1));
        assert!(w.all_day);
        assert_eq!(parse("1/2 litre de lait"), None);
        assert_eq!(parse("le 31 février réunion à 10h"), None);
        assert_eq!(parse("12/11 14h dentiste").unwrap().start, p(2026, 11, 12, 14, 0));
    }

    #[test]
    fn nothing_temporal() {
        assert_eq!(parse("rien de temporel ici"), None);
    }

    #[test]
    fn default_hour() {
        assert_eq!(DEFAULT_REMINDER_HOUR, 9);
        let d = When {
            start: Local.with_ymd_and_hms(2026, 11, 9, 0, 0, 0).unwrap(),
            end: None,
            all_day: true,
            rest: String::new(),
        };
        assert_eq!(at_default_hour(&d).hour(), 9);
        let timed = When { start: d.start + Duration::hours(15), all_day: false, ..d };
        assert_eq!(at_default_hour(&timed), timed.start);
    }
}
