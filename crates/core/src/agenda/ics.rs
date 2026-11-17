//! Lecture iCalendar (RFC 5545) : VEVENT seulement, récurrences simples développées sur une fenêtre bornée.
//! Contenu non fiable : taille, nombre d'occurrences et longueur des textes bornés ; jamais journalisé.
use crate::CoreError;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;
use std::collections::{HashMap, HashSet};

pub const MAX_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_OCCURRENCES: usize = 20_000;
const MAX_TITLE: usize = 200;
const MAX_NOTES: usize = 2_000;
const MAX_UID: usize = 300;
/// Périodes examinées au plus par récurrence, et pour tout le fichier : une règle étrange ne fait jamais tourner en rond.
const MAX_PERIODS: u32 = 5_000;
const MAX_PERIODS_TOTAL: u32 = 2_000_000;
const DAY_MS: i64 = 86_400_000;

/// Fenêtre développée : un an en arrière, deux ans en avant.
pub fn window(now_ms: i64) -> (i64, i64) { (now_ms - 365 * DAY_MS, now_ms + 730 * DAY_MS) }

#[derive(Clone, PartialEq)]
pub struct IcsEvent {
    pub uid: Option<String>,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub location: Option<String>,
    pub notes: Option<String>,
}

impl std::fmt::Debug for IcsEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IcsEvent").field("start_ms", &self.start_ms).field("end_ms", &self.end_ms).finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct Parsed {
    /// `X-WR-CALNAME`.
    pub name: Option<String>,
    pub events: Vec<IcsEvent>,
    /// Plus de `MAX_OCCURRENCES` occurrences : la suite est ignorée.
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Zone { Date, Utc, Tz(Tz), Floating }

#[derive(Clone, Copy, Debug)]
struct Dt { at: NaiveDateTime, zone: Zone }

fn at_ms(at: NaiveDateTime, zone: Zone) -> Option<i64> {
    match zone {
        Zone::Utc => Some(Utc.from_utc_datetime(&at).timestamp_millis()),
        Zone::Tz(tz) => tz.from_local_datetime(&at).earliest().or_else(|| tz.from_local_datetime(&(at + Duration::hours(1))).earliest()).map(|t| t.timestamp_millis()),
        // Date seule ou heure « flottante » : heure locale (minuit qui n'existe pas : une heure plus tard).
        Zone::Date | Zone::Floating => Local.from_local_datetime(&at).earliest().or_else(|| Local.from_local_datetime(&(at + Duration::hours(1))).earliest()).map(|t| t.timestamp_millis()),
    }
}

struct Prop<'a> { name: String, params: Vec<(String, &'a str)>, value: &'a str }

impl Prop<'_> {
    fn param(&self, k: &str) -> Option<&str> { self.params.iter().find(|(n, _)| n == k).map(|(_, v)| *v) }
}

/// Coupe sur `sep` hors des guillemets.
fn split_unquoted(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), 0, false);
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            out.push(&s[start..i]);
            start = i + 1;
        }
    }
    out.push(&s[start..]);
    out
}

/// `NOM;PARAM=VAL:valeur`.
fn prop(line: &str) -> Option<Prop<'_>> {
    let mut quoted = false;
    let colon = line.char_indices().find(|&(_, c)| {
        if c == '"' { quoted = !quoted; }
        c == ':' && !quoted
    })?.0;
    let mut parts = split_unquoted(&line[..colon], ';').into_iter();
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts.filter_map(|p| p.split_once('=')).map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim().trim_matches('"'))).collect();
    Some(Prop { name, params, value: &line[colon + 1..] })
}

/// Lignes dépliées (une ligne qui commence par une espace ou une tabulation continue la précédente).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        if let (Some(rest), Some(last)) = (l.strip_prefix([' ', '\t']), out.last_mut()) {
            last.push_str(rest);
        } else if !l.trim().is_empty() {
            out.push(l.to_string());
        }
    }
    out
}

/// Texte iCalendar déséchappé, sans caractère de contrôle (sauf saut de ligne), borné à `max` caractères.
fn text(v: &str, max: usize) -> String {
    let (mut out, mut n, mut it) = (String::new(), 0, v.chars());
    while let Some(c) = it.next() {
        let c = if c == '\\' { match it.next() { Some('n' | 'N') => '\n', Some(o) => o, None => break } } else { c };
        if c.is_control() && c != '\n' {
            continue;
        }
        if n == max {
            break;
        }
        out.push(c);
        n += 1;
    }
    out.trim().to_string()
}

fn opt_text(v: &str, max: usize) -> Option<String> { Some(text(v, max)).filter(|t| !t.is_empty()) }

/// Fuseau IANA, y compris préfixé (`/mozilla.org/20050126_1/Europe/Paris`).
// shortcut: les noms de fuseau Windows (« Romance Standard Time ») tombent sur l'heure locale ; les traduire si un export Outlook pose problème.
fn tz_of(id: &str) -> Option<Tz> {
    let segs: Vec<&str> = id.split('/').filter(|s| !s.is_empty()).collect();
    (0..segs.len()).find_map(|i| segs[i..].join("/").parse().ok())
}

fn parse_dt(v: &str, tzid: Option<&str>) -> Option<Dt> {
    let v = v.trim();
    if v.len() == 8 {
        return Some(Dt { at: NaiveDate::parse_from_str(v, "%Y%m%d").ok()?.and_hms_opt(0, 0, 0)?, zone: Zone::Date });
    }
    let (body, utc) = v.strip_suffix(['Z', 'z']).map_or((v, false), |b| (b, true));
    let at = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    let zone = if utc { Zone::Utc } else { tzid.and_then(tz_of).map_or(Zone::Floating, Zone::Tz) };
    Some(Dt { at, zone })
}

/// `P1W`, `P2DT3H`, `-PT15M`…
fn duration(v: &str) -> Option<Duration> {
    let v = v.trim();
    let (neg, v) = match v.strip_prefix('-') { Some(r) => (true, r), None => (false, v.strip_prefix('+').unwrap_or(v)) };
    let (mut total, mut num, mut time) = (Duration::zero(), String::new(), false);
    for c in v.strip_prefix('P')?.chars() {
        match c {
            '0'..='9' if num.len() < 6 => num.push(c),
            'T' => time = true,
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = std::mem::take(&mut num).parse().ok()?;
                total += match (c, time) {
                    ('W', false) => Duration::weeks(n),
                    ('D', false) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    Some(if neg { -total } else { total })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Freq { Daily, Weekly, Monthly, Yearly }

#[derive(Debug)]
struct Rule { freq: Freq, interval: u32, count: Option<u32>, until: Option<Dt>, by_day: Vec<(i32, Weekday)>, by_month: Vec<u32>, by_month_day: Vec<i32> }

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s { "MO" => Weekday::Mon, "TU" => Weekday::Tue, "WE" => Weekday::Wed, "TH" => Weekday::Thu, "FR" => Weekday::Fri, "SA" => Weekday::Sat, "SU" => Weekday::Sun, _ => return None })
}

/// `None` : règle non prise en charge (BYSETPOS, BYHOUR…) ; seule la première occurrence est gardée.
fn rule(v: &str) -> Option<Rule> {
    let mut r = Rule { freq: Freq::Daily, interval: 1, count: None, until: None, by_day: vec![], by_month: vec![], by_month_day: vec![] };
    let mut freq = None;
    for part in v.trim().split(';').filter(|p| !p.is_empty()) {
        let (k, val) = part.split_once('=')?;
        let val = val.trim().to_ascii_uppercase();
        let list = || val.split(',').map(str::trim).filter(|s| !s.is_empty());
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => freq = Some(match val.as_str() { "DAILY" => Freq::Daily, "WEEKLY" => Freq::Weekly, "MONTHLY" => Freq::Monthly, "YEARLY" => Freq::Yearly, _ => return None }),
            "INTERVAL" => r.interval = val.parse().ok().filter(|n| (1..=1000).contains(n))?,
            "COUNT" => r.count = Some(val.parse().ok()?),
            "UNTIL" => r.until = Some(parse_dt(&val, None)?),
            "WKST" => {}
            "BYDAY" => for d in list() {
                let (n, wd) = d.split_at(d.len().checked_sub(2)?);
                let n: i32 = if n.is_empty() { 0 } else { n.parse().ok().filter(|n: &i32| *n != 0 && n.abs() <= 5)? };
                r.by_day.push((n, weekday(wd)?));
            },
            "BYMONTH" => for m in list() { r.by_month.push(m.parse().ok().filter(|m| (1..=12).contains(m))?); },
            "BYMONTHDAY" => for d in list() { r.by_month_day.push(d.parse().ok().filter(|d: &i32| *d != 0 && d.abs() <= 31)?); },
            _ => return None,
        }
    }
    r.freq = freq?;
    // Combinaisons rares (ordinal sur l'année, BYDAY croisé avec BYMONTHDAY) : non prises en charge.
    let unsupported = (!r.by_day.is_empty() && !r.by_month_day.is_empty())
        || (r.freq == Freq::Yearly && r.by_month.is_empty() && (!r.by_day.is_empty() || !r.by_month_day.is_empty()))
        || (matches!(r.freq, Freq::Daily | Freq::Weekly) && (!r.by_month_day.is_empty() || r.by_day.iter().any(|d| d.0 != 0)));
    (!unsupported).then_some(r)
}

fn month_len(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1).and_then(|d| d.pred_opt()).map_or(28, |d| d.day())
}

/// Jours du mois retenus par la règle, triés.
fn month_dates(y: i32, m: u32, r: &Rule, default_day: u32) -> Vec<NaiveDate> {
    let len = month_len(y, m);
    let mut days: Vec<u32> = Vec::new();
    if !r.by_month_day.is_empty() {
        days.extend(r.by_month_day.iter().map(|&d| if d > 0 { d } else { len as i32 + 1 + d }).filter(|d| (1..=len as i32).contains(d)).map(|d| d as u32));
    } else if !r.by_day.is_empty() {
        for &(n, wd) in &r.by_day {
            let all: Vec<u32> = (1..=len).filter(|&d| NaiveDate::from_ymd_opt(y, m, d).is_some_and(|x| x.weekday() == wd)).collect();
            match n {
                0 => days.extend(&all),
                n if n > 0 => days.extend(all.get(n as usize - 1)),
                n => days.extend(all.len().checked_sub(n.unsigned_abs() as usize).and_then(|i| all.get(i))),
            }
        }
    } else if default_day <= len {
        days.push(default_day);
    }
    days.sort_unstable();
    days.dedup();
    days.into_iter().filter_map(|d| NaiveDate::from_ymd_opt(y, m, d)).collect()
}

/// Dates de la `k`-ième période de la règle.
fn period_dates(r: &Rule, start: NaiveDate, k: u32) -> Option<Vec<NaiveDate>> {
    let step = k as i64 * r.interval as i64;
    Some(match r.freq {
        Freq::Daily => {
            let d = start.checked_add_signed(Duration::days(step))?;
            if r.by_day.is_empty() || r.by_day.iter().any(|x| x.1 == d.weekday()) { vec![d] } else { vec![] }
        }
        Freq::Weekly => {
            let monday = start.checked_sub_signed(Duration::days(start.weekday().num_days_from_monday() as i64))?.checked_add_signed(Duration::weeks(step))?;
            let mut wds: Vec<u32> = if r.by_day.is_empty() { vec![start.weekday().num_days_from_monday()] } else { r.by_day.iter().map(|d| d.1.num_days_from_monday()).collect() };
            wds.sort_unstable();
            wds.dedup();
            wds.into_iter().filter_map(|w| monday.checked_add_signed(Duration::days(w as i64))).collect()
        }
        Freq::Monthly => {
            let total = start.year() as i64 * 12 + start.month0() as i64 + step;
            month_dates(i32::try_from(total.div_euclid(12)).ok()?, total.rem_euclid(12) as u32 + 1, r, start.day())
        }
        Freq::Yearly => {
            let y = i32::try_from(start.year() as i64 + step).ok()?;
            let months = if r.by_month.is_empty() { vec![start.month()] } else { let mut m = r.by_month.clone(); m.sort_unstable(); m.dedup(); m };
            months.into_iter().flat_map(|m| month_dates(y, m, r, start.day())).collect()
        }
    })
}

/// Début de chaque occurrence (heure locale de l'événement et instant), DTSTART compris, jusqu'à `to_ms` exclu.
/// Sans COUNT, les périodes bien avant `from_ms` sont sautées (série ouverte depuis des années).
fn occurrences(start: Dt, r: &Rule, from_ms: i64, to_ms: i64, budget: &mut u32) -> Vec<(NaiveDateTime, i64)> {
    let until = r.until.and_then(|u| if u.zone == Zone::Date { to_ms_end_of_day(u.at) } else { to_ms_of(u, start.zone) });
    let mut out = Vec::new();
    let max = r.count.map_or(MAX_OCCURRENCES, |c| (c as usize).min(MAX_OCCURRENCES));
    let push = |at: NaiveDateTime, out: &mut Vec<(NaiveDateTime, i64)>| -> bool {
        let Some(ms) = at_ms(at, start.zone) else { return true };
        if ms >= to_ms || until.is_some_and(|u| ms > u) || out.len() >= max {
            return false;
        }
        out.push((at, ms));
        true
    };
    if !push(start.at, &mut out) {
        return out;
    }
    let first = if r.count.is_some() { 0 } else {
        let days = (from_ms - at_ms(start.at, start.zone).unwrap_or(from_ms)) / DAY_MS;
        let len = r.interval as i64 * match r.freq { Freq::Daily => 1, Freq::Weekly => 7, Freq::Monthly => 28, Freq::Yearly => 365 };
        u32::try_from((days / len - 1).max(0)).unwrap_or(u32::MAX)
    };
    for k in first..first.saturating_add(MAX_PERIODS) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let Some(dates) = period_dates(r, start.at.date(), k) else { break };
        for d in dates {
            let at = d.and_time(start.at.time());
            if at > start.at && !push(at, &mut out) {
                return out;
            }
        }
    }
    out
}

fn to_ms_end_of_day(at: NaiveDateTime) -> Option<i64> { Some(at_ms(at.date().succ_opt()?.and_hms_opt(0, 0, 0)?, Zone::Date)? - 1) }

/// UNTIL en heure flottante : lu dans le fuseau de DTSTART.
fn to_ms_of(u: Dt, start_zone: Zone) -> Option<i64> { at_ms(u.at, if u.zone == Zone::Floating { start_zone } else { u.zone }) }

#[derive(Default)]
struct Raw {
    uid: Option<String>,
    summary: Option<String>,
    location: Option<String>,
    description: Option<String>,
    start: Option<Dt>,
    end: Option<Dt>,
    duration: Option<Duration>,
    rrule: Option<String>,
    exdates: Vec<Dt>,
    recurrence_id: Option<Dt>,
    cancelled: bool,
}

/// Lit un fichier .ics et développe ses événements qui chevauchent [from_ms, to_ms[.
pub fn parse(text: &str, from_ms: i64, to_ms: i64) -> Result<Parsed, CoreError> {
    if text.len() > MAX_BYTES {
        return Err(CoreError::Invalid("fichier trop volumineux (5 Mo au maximum)".into()));
    }
    let lines = unfold(text.trim_start_matches('\u{feff}'));
    if !lines.first().is_some_and(|l| l.trim().eq_ignore_ascii_case("BEGIN:VCALENDAR")) {
        return Err(CoreError::Invalid("ce fichier n'est pas un calendrier .ics".into()));
    }
    let (mut name, mut raws, mut stack) = (None, Vec::<Raw>::new(), Vec::<String>::new());
    let mut cur = Raw::default();
    for l in &lines {
        let Some(p) = prop(l) else { continue };
        match p.name.as_str() {
            "BEGIN" => {
                if stack.len() >= 8 {
                    return Err(CoreError::Invalid("calendrier mal formé".into()));
                }
                stack.push(p.value.trim().to_ascii_uppercase());
                if stack == ["VCALENDAR", "VEVENT"] {
                    cur = Raw::default();
                }
                continue;
            }
            "END" => {
                if stack == ["VCALENDAR", "VEVENT"] {
                    raws.push(std::mem::take(&mut cur));
                    if raws.len() > MAX_OCCURRENCES {
                        return Err(CoreError::Invalid("trop d'événements dans ce calendrier".into()));
                    }
                }
                stack.pop();
                continue;
            }
            _ => {}
        }
        if stack == ["VCALENDAR"] && p.name == "X-WR-CALNAME" {
            name = opt_text(p.value, 80);
        }
        if stack != ["VCALENDAR", "VEVENT"] {
            continue; // VTIMEZONE, VALARM, VTODO… : ignorés
        }
        let tzid = p.param("TZID");
        match p.name.as_str() {
            "UID" => cur.uid = opt_text(p.value, MAX_UID),
            "SUMMARY" => cur.summary = opt_text(p.value, MAX_TITLE),
            "LOCATION" => cur.location = opt_text(p.value, MAX_TITLE),
            "DESCRIPTION" => cur.description = opt_text(p.value, MAX_NOTES),
            "DTSTART" => cur.start = parse_dt(p.value, tzid),
            "DTEND" => cur.end = parse_dt(p.value, tzid),
            "DURATION" => cur.duration = duration(p.value),
            "RRULE" => cur.rrule = Some(p.value.to_string()),
            "EXDATE" => cur.exdates.extend(p.value.split(',').filter_map(|v| parse_dt(v, tzid)).take(1000)),
            "RECURRENCE-ID" => cur.recurrence_id = parse_dt(p.value, tzid),
            "STATUS" => cur.cancelled = p.value.trim().eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    // Occurrences remplacées (RECURRENCE-ID) : retirées de la série, la version modifiée est un événement à part.
    let mut replaced: HashMap<String, HashSet<i64>> = HashMap::new();
    for r in &raws {
        if let (Some(uid), Some(rid)) = (&r.uid, r.recurrence_id) {
            replaced.entry(uid.clone()).or_default().extend(at_ms(rid.at, rid.zone));
        }
    }
    let mut out = Parsed { name, ..Default::default() };
    let mut budget = MAX_PERIODS_TOTAL;
    for r in raws {
        let Some(start) = r.start else { continue };
        if r.cancelled {
            continue;
        }
        let all_day = start.zone == Zone::Date;
        let Some(start_ms) = at_ms(start.at, start.zone) else { continue };
        // Durée : en jours pour une journée entière (minuit à minuit, même au changement d'heure), sinon en ms.
        let days = if all_day {
            r.end.map(|e| (e.at.date() - start.at.date()).num_days()).or(r.duration.map(|d| d.num_days())).filter(|d| *d >= 1).unwrap_or(1)
        } else { 0 };
        let dur_ms = if all_day { 0 } else {
            r.end.and_then(|e| at_ms(e.at, if e.zone == Zone::Floating { start.zone } else { e.zone })).map(|e| e - start_ms)
                .or(r.duration.map(|d| d.num_milliseconds())).filter(|d| *d >= 0).unwrap_or(0)
        };
        let mut skip: HashSet<i64> = r.exdates.iter().filter_map(|d| to_ms_of(*d, start.zone)).collect();
        if r.recurrence_id.is_none() {
            if let Some(set) = r.uid.as_ref().and_then(|u| replaced.get(u)) {
                skip.extend(set);
            }
        }
        let starts = match r.rrule.as_deref().filter(|_| r.recurrence_id.is_none()).and_then(rule) {
            Some(rule) => occurrences(start, &rule, from_ms, to_ms, &mut budget),
            None => vec![(start.at, start_ms)],
        };
        let title = r.summary.unwrap_or_else(|| "(Sans titre)".into());
        for (at, s) in starts {
            if skip.contains(&s) {
                continue;
            }
            let e = if all_day { at.checked_add_signed(Duration::days(days)).and_then(|x| at_ms(x, Zone::Date)) } else { Some(s + dur_ms) };
            let Some(e) = e else { continue };
            let overlaps = s < to_ms && (e > from_ms || (e == s && s >= from_ms));
            if !overlaps {
                continue;
            }
            if out.events.len() >= MAX_OCCURRENCES {
                out.truncated = true;
                return Ok(out);
            }
            out.events.push(IcsEvent { uid: r.uid.clone(), title: title.clone(), start_ms: s, end_ms: e, all_day, location: r.location.clone(), notes: r.description.clone() });
        }
    }
    out.events.sort_by_key(|e| e.start_ms);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, m: u32, d: u32, h: u32, mi: u32) -> i64 {
        Local.from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, 0).unwrap()).earliest().unwrap().timestamp_millis()
    }
    fn cal(body: &str) -> String { format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Club\\, saison\r\n{body}END:VCALENDAR\r\n") }
    fn ev(lines: &str) -> String { format!("BEGIN:VEVENT\r\n{lines}END:VEVENT\r\n") }
    const ALL: (i64, i64) = (0, i64::MAX / 2);
    fn starts(p: &Parsed) -> Vec<i64> { p.events.iter().map(|e| e.start_ms).collect() }

    #[test]
    fn simple_utc_tzid_et_date_seule() {
        let txt = cal(&[
            ev("UID:a@x\r\nSUMMARY:Match\\, aller\r\nDTSTART:20261114T130000Z\r\nDTEND:20261114T150000Z\r\nLOCATION:Stade\r\nDESCRIPTION:Ligne 1\\nLigne 2\r\n"),
            ev("SUMMARY:Réunion\r\nDTSTART;TZID=America/New_York:20261113T090000\r\nDURATION:PT1H30M\r\n"),
            ev("SUMMARY:Armistice\r\nDTSTART;VALUE=DATE:20261111\r\n"),
            ev("SUMMARY:Mozilla\r\nDTSTART;TZID=/mozilla.org/20050126_1/Europe/Paris:20261120T100000\r\n"),
            "BEGIN:VTIMEZONE\r\nTZID:X\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n".into(),
        ].concat());
        let p = parse(&txt, ALL.0, ALL.1).unwrap();
        assert_eq!(p.name.as_deref(), Some("Club, saison"));
        assert_eq!(p.events.len(), 4, "le VTIMEZONE n'est pas un événement");
        let by = |t: &str| p.events.iter().find(|e| e.title == t).unwrap();
        let m = by("Match, aller");
        assert_eq!((m.start_ms, m.end_ms - m.start_ms), (Utc.with_ymd_and_hms(2026, 11, 14, 13, 0, 0).unwrap().timestamp_millis(), 2 * 3_600_000));
        assert_eq!((m.uid.as_deref(), m.location.as_deref(), m.notes.as_deref()), (Some("a@x"), Some("Stade"), Some("Ligne 1\nLigne 2")));
        let r = by("Réunion");
        assert_eq!(r.start_ms, chrono_tz::America::New_York.with_ymd_and_hms(2026, 11, 13, 9, 0, 0).unwrap().timestamp_millis());
        assert_eq!(r.end_ms - r.start_ms, 90 * 60_000);
        let a = by("Armistice");
        assert!(a.all_day);
        assert_eq!((a.start_ms, a.end_ms), (local(2026, 11, 11, 0, 0), local(2026, 11, 12, 0, 0)));
        assert_eq!(by("Mozilla").start_ms, chrono_tz::Europe::Paris.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap().timestamp_millis());
    }

    #[test]
    fn lignes_pliees_valarm_et_texte_borne() {
        let long = "x".repeat(500);
        let txt = cal(&ev(&format!("SUMMARY:Dé\r\n but\r\nDTSTART:20261201T080000\r\nDESCRIPTION:{long}\r\nBEGIN:VALARM\r\nDESCRIPTION:alarme\r\nDTSTART:20000101T000000\r\nEND:VALARM\r\n")));
        let p = parse(&txt, ALL.0, ALL.1).unwrap();
        assert_eq!(p.events.len(), 1);
        assert_eq!(p.events[0].title, "Début");
        assert_eq!(p.events[0].start_ms, local(2026, 12, 1, 8, 0), "heure flottante : heure locale");
        assert_eq!(p.events[0].notes.as_ref().unwrap().len(), 500, "la description de l'alarme n'écrase pas celle de l'événement");
        let txt = cal(&ev(&format!("SUMMARY:{}\r\nDTSTART:20261201T080000\r\n", "é".repeat(400))));
        assert_eq!(parse(&txt, ALL.0, ALL.1).unwrap().events[0].title.chars().count(), MAX_TITLE);
    }

    #[test]
    fn hebdomadaire_byday_count_et_exdate() {
        // Lundi 2 novembre 2026 : lundis et mercredis, 5 occurrences, le mercredi 4 retiré.
        let txt = cal(&ev("SUMMARY:Sport\r\nDTSTART;TZID=Europe/Paris:20261102T180000\r\nDTEND;TZID=Europe/Paris:20261102T190000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=5\r\nEXDATE;TZID=Europe/Paris:20261104T180000\r\n"));
        let p = parse(&txt, ALL.0, ALL.1).unwrap();
        let paris = |d: u32| chrono_tz::Europe::Paris.with_ymd_and_hms(2026, 11, d, 18, 0, 0).unwrap().timestamp_millis();
        assert_eq!(starts(&p), vec![paris(2), paris(9), paris(11), paris(16)]);
        assert!(p.events.iter().all(|e| e.end_ms - e.start_ms == 3_600_000));
    }

    #[test]
    fn quotidien_until_intervalle_et_changement_d_heure() {
        // Tous les 2 jours jusqu'au 1er novembre inclus, en traversant le passage à l'heure d'hiver (25 octobre).
        let txt = cal(&ev("SUMMARY:x\r\nDTSTART;TZID=Europe/Paris:20261021T090000\r\nRRULE:FREQ=DAILY;INTERVAL=2;UNTIL=20261101T080000Z\r\n"));
        let p = parse(&txt, ALL.0, ALL.1).unwrap();
        let paris = |m: u32, d: u32| chrono_tz::Europe::Paris.with_ymd_and_hms(2026, m, d, 9, 0, 0).unwrap().timestamp_millis();
        assert_eq!(starts(&p), vec![paris(10, 21), paris(10, 23), paris(10, 25), paris(10, 27), paris(10, 29), paris(10, 31)]);
    }

    #[test]
    fn mensuel_et_annuel() {
        // Le 31 : seuls les mois qui en ont un. Dernier vendredi. Annuel jour entier. 4e jeudi de novembre.
        let txt = cal(&[
            ev("UID:m31\r\nSUMMARY:fin\r\nDTSTART;VALUE=DATE:20260131\r\nRRULE:FREQ=MONTHLY;COUNT=4\r\n"),
            ev("UID:vend\r\nSUMMARY:vendredi\r\nDTSTART:20261127T120000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=3\r\n"),
            ev("UID:anniv\r\nSUMMARY:anniv\r\nDTSTART;VALUE=DATE:20240229\r\nRRULE:FREQ=YEARLY\r\n"),
            ev("UID:tg\r\nSUMMARY:thanksgiving\r\nDTSTART;VALUE=DATE:20251127\r\nRRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=4TH;UNTIL=20271231\r\n"),
        ].concat());
        let p = parse(&txt, ALL.0, local(2032, 1, 1, 0, 0)).unwrap();
        let of = |uid: &str| p.events.iter().filter(|e| e.uid.as_deref() == Some(uid)).map(|e| e.start_ms).collect::<Vec<_>>();
        let d = |y, m, dd| local(y, m, dd, 0, 0);
        assert_eq!(of("m31"), vec![d(2026, 1, 31), d(2026, 3, 31), d(2026, 5, 31), d(2026, 7, 31)]);
        let noon = |m: u32, dd: u32, y: i32| Utc.with_ymd_and_hms(y, m, dd, 12, 0, 0).unwrap().timestamp_millis();
        assert_eq!(of("vend"), vec![noon(11, 27, 2026), noon(12, 25, 2026), noon(1, 29, 2027)]);
        assert_eq!(of("anniv"), vec![d(2024, 2, 29), d(2028, 2, 29)], "29 février : années bissextiles seulement");
        assert_eq!(of("tg"), vec![d(2025, 11, 27), d(2026, 11, 26), d(2027, 11, 25)]);
    }

    #[test]
    fn fenetre_occurrence_remplacee_annulee_et_regle_inconnue() {
        let txt = cal(&[
            ev("UID:s\r\nSUMMARY:série\r\nDTSTART:20261102T100000Z\r\nRRULE:FREQ=DAILY\r\n"),
            ev("UID:s\r\nSUMMARY:série déplacée\r\nRECURRENCE-ID:20261104T100000Z\r\nDTSTART:20261104T150000Z\r\n"),
            ev("UID:c\r\nSUMMARY:annulé\r\nSTATUS:CANCELLED\r\nDTSTART:20261103T100000Z\r\n"),
            ev("UID:u\r\nSUMMARY:inconnue\r\nDTSTART:20261103T100000Z\r\nRRULE:FREQ=MONTHLY;BYSETPOS=-1;BYDAY=MO\r\n"),
            ev("SUMMARY:sans début\r\n"),
        ].concat());
        let at = |d: u32, h: u32| Utc.with_ymd_and_hms(2026, 11, d, h, 0, 0).unwrap().timestamp_millis();
        let p = parse(&txt, at(3, 0), at(6, 0)).unwrap();
        let got: Vec<(&str, i64)> = p.events.iter().map(|e| (e.title.as_str(), e.start_ms)).collect();
        assert_eq!(got, vec![("série", at(3, 10)), ("inconnue", at(3, 10)), ("série déplacée", at(4, 15)), ("série", at(5, 10))]);
    }

    #[test]
    fn serie_sans_fin_bornee_et_fichiers_refuses() {
        let txt = cal(&ev("SUMMARY:x\r\nDTSTART:20000101T000000Z\r\nRRULE:FREQ=DAILY\r\n"));
        let p = parse(&txt, ALL.0, ALL.1).unwrap();
        assert!(p.events.len() <= MAX_OCCURRENCES);
        assert!(parse("<html></html>", 0, 1).is_err());
        assert!(parse(&"x".repeat(MAX_BYTES + 1), 0, 1).is_err());
        assert!(parse("BEGIN:VCALENDAR\nEND:VCALENDAR", 0, 1).unwrap().events.is_empty());
        // Règles mal formées : seule la première occurrence.
        for r in ["FREQ=HOURLY", "FREQ=DAILY;INTERVAL=0", "FREQ=WEEKLY;BYDAY=XX", "nimportequoi"] {
            let p = parse(&cal(&ev(&format!("SUMMARY:x\r\nDTSTART:20261102T100000Z\r\nRRULE:{r}\r\n"))), ALL.0, ALL.1).unwrap();
            assert_eq!(p.events.len(), 1, "{r}");
        }
    }

    #[test]
    fn durees() {
        assert_eq!(duration("P1W"), Some(Duration::weeks(1)));
        assert_eq!(duration("P1DT2H3M4S"), Some(Duration::days(1) + Duration::hours(2) + Duration::minutes(3) + Duration::seconds(4)));
        assert_eq!(duration("-PT15M"), Some(-Duration::minutes(15)));
        assert_eq!(duration("P1H"), None);
        assert_eq!(duration("P99999999D"), None);
    }
}
