//! Briefing d'IRIS : faits déterministes lus en base, texte en français avec références (les titres externes ne
//! passent jamais en clair), reformulation facultative par le modèle local, actions choisies par le code seul.
use super::ollama::LocalBrain;
use super::taint::{render, Segment, Vault};
use crate::projects::Project;
use crate::time_util::local_day_bounds;
use crate::weather::Weather;
use chrono::{Local, TimeZone, Timelike};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::hash::{Hash, Hasher};

/// Clé `kv` du dernier briefing.
pub const KEY: &str = "briefing";
/// Délai minimal entre deux calculs le même jour.
pub const MIN_GAP_MS: i64 = 15 * 60_000;
/// Taille maximale d'une reformulation acceptée.
const POLISH_MAX_CHARS: usize = 400;
const MAX_SENTENCES: usize = 3;
/// Une météo plus vieille que cela n'alerte plus.
const WEATHER_FRESH_MS: i64 = 3 * 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Facts {
    pub todo_mails: u32,
    /// (compte, id) du mail à traiter le plus récent.
    pub top_todo: Option<(i64, String)>,
    /// Expéditeur de ce mail (étiquette de l'action seulement, jamais dans le texte du briefing).
    pub top_from: Option<String>,
    /// (début ms, titre, externe) du prochain événement horaire d'aujourd'hui.
    pub next_event: Option<(i64, String, bool)>,
    pub reminders_today: u32,
    /// (nom, étape en cours, total) du projet au plan en cours le plus récemment actif.
    pub project: Option<(String, u32, u32)>,
    pub weather_alert: Option<String>,
}

pub fn facts(c: &Connection, projects: &[Project], now: i64) -> Facts {
    let todo_mails = c.query_row("SELECT COUNT(*) FROM mails WHERE bucket = 'todo'", [], |r| r.get(0)).unwrap_or(0);
    let top = c.query_row(
        "SELECT account_id, id, from_name, from_addr FROM mails WHERE bucket = 'todo' ORDER BY date_ms DESC LIMIT 1", [],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
    ).ok();
    let (day_start, day_end) = local_day_bounds(now);
    let next_event = crate::agenda::between(c, now.max(day_start), day_end).unwrap_or_default().into_iter()
        .find(|e| !e.all_day && e.start_ms >= now)
        .map(|e| (e.start_ms, e.title, e.readonly));
    let reminders_today = crate::reminders::list_open(c).unwrap_or_default().iter().filter(|r| r.due_ms.is_some_and(|d| d < day_end)).count() as u32;
    let project = projects.iter()
        .filter(|p| !p.hidden && p.plan.as_ref().is_some_and(|x| x.done < x.total))
        .max_by_key(|p| p.last_activity_ms)
        .and_then(|p| p.plan.as_ref().map(|x| (p.name.clone(), x.done + 1, x.total)));
    let weather_alert = crate::kv::get(c, crate::weather::tools::LAST_KEY).ok().flatten()
        .and_then(|(json, _)| serde_json::from_str::<Weather>(&json).ok())
        .filter(|w| now - w.fetched_ms < WEATHER_FRESH_MS)
        .and_then(|w| w.rain_from_hour)
        .map(|h| format!("Pluie prévue dès {h} h"));
    Facts {
        todo_mails,
        top_from: top.as_ref().map(|t| if t.2.is_empty() { t.3.clone() } else { t.2.clone() }),
        top_todo: top.map(|t| (t.0, t.1)),
        next_event, reminders_today, project, weather_alert,
    }
}

pub fn hash(f: &Facts) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.hash(&mut h);
    h.finish()
}

fn hm(ms: i64) -> String {
    let t = Local.timestamp_millis_opt(ms).earliest().unwrap_or_default();
    if t.minute() == 0 { format!("{} h", t.hour()) } else { format!("{} h {:02}", t.hour(), t.minute()) }
}

fn compose(f: &Facts, name: &str, put: &mut dyn FnMut(&str, String) -> String) -> String {
    let mut s: Vec<(&str, String)> = Vec::new();
    match f.todo_mails {
        0 => {}
        1 => s.push(("mails", "1 mail à traiter.".into())),
        n => s.push(("mails", format!("{n} mails à traiter."))),
    }
    if let Some((start, title, external)) = &f.next_event {
        let title = if *external { put("agenda externe", title.clone()) } else { title.clone() };
        s.push(("event", format!("Prochain : {title} à {}.", hm(*start))));
    }
    if let Some((p, step, total)) = &f.project {
        s.push(("project", format!("Sur {p}, tu en es à l'étape {step} sur {total}.")));
    }
    match f.reminders_today {
        0 => {}
        1 => s.push(("reminders", "1 rappel aujourd'hui.".into())),
        n => s.push(("reminders", format!("{n} rappels aujourd'hui."))),
    }
    if let Some(w) = &f.weather_alert {
        s.push(("weather", format!("{w}.")));
    }
    for drop in ["reminders", "weather", "project"] {
        if s.len() > MAX_SENTENCES {
            s.retain(|(k, _)| *k != drop);
        }
    }
    if s.is_empty() {
        return if name.is_empty() { "Rien d'urgent. Bonne journée.".into() } else { format!("Rien d'urgent. Bonne journée, {name}.") };
    }
    s.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join(" ")
}

/// Texte déterministe (côté planificateur : références `{{uN}}` dans `vault`).
pub fn text(f: &Facts, vault: &mut Vault, name: &str) -> String {
    compose(f, name, &mut |src, t| vault.put(src, t))
}

/// Références `{{uN}}` d'un texte, dans l'ordre, sans doublon.
fn refs_of(t: &str) -> Vec<String> {
    let (mut out, mut i) = (Vec::<String>::new(), 0);
    while let Some(off) = t[i..].find("{{u") {
        let s = i + off;
        let d = s + 3;
        let e = d + t[d..].bytes().take_while(u8::is_ascii_digit).count();
        if e > d && t[e..].starts_with("}}") {
            let r = t[s..e + 2].to_string();
            if !out.contains(&r) { out.push(r); }
            i = e + 2;
        } else {
            i = d;
        }
    }
    out
}

fn sentences(t: &str) -> usize { t.split(['.', '!', '?', '…']).filter(|p| !p.trim().is_empty()).count() }

/// Reformulation au ton d'IRIS ; `None` (on garde le brouillon) si le modèle échoue, invente ou perd une référence,
/// ou dépasse 400 caractères / 3 phrases.
pub async fn polish(local: &dyn LocalBrain, draft: &str, personality: &str) -> Option<String> {
    let system = format!(
        "{personality}\nReformule ces phrases à ta façon, en 3 phrases au plus et 400 caractères au plus. Garde chaque référence de la forme {{{{u1}}}} exactement telle quelle, n'en ajoute aucune, n'ajoute aucune information."
    );
    let schema = json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] });
    let out = local.structured(&system, draft, &schema).await.ok()?;
    let t = out["text"].as_str()?.trim().to_string();
    let (a, b) = (refs_of(draft), refs_of(&t));
    let same = a.len() == b.len() && a.iter().all(|r| b.contains(r));
    (same && !t.is_empty() && t.chars().count() <= POLISH_MAX_CHARS && sentences(&t) <= MAX_SENTENCES).then_some(t)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode { Normal, Orchestrator }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Suggestion {
    ReplyMail { account: i64, id: String, label: String },
    ResumeProject { name: String, mode: Mode, label: String },
    OpenTodo { label: String },
    StartFocus { label: String },
}

/// Deux ou trois actions, choisies d'après les faits seuls.
pub fn suggestions(f: &Facts, now: i64) -> Vec<Suggestion> {
    let mut out = Vec::new();
    if let Some((account, id)) = &f.top_todo {
        let from = f.top_from.as_deref().unwrap_or("l'expéditeur");
        out.push(Suggestion::ReplyMail { account: *account, id: id.clone(), label: format!("Préparer la réponse à {from}") });
    }
    if let Some((name, _, _)) = &f.project {
        out.push(Suggestion::ResumeProject { name: name.clone(), mode: Mode::Orchestrator, label: format!("Reprendre {name} · orchestrateur") });
    }
    if f.next_event.as_ref().is_none_or(|(start, _, _)| *start - now > 3_600_000) {
        out.push(Suggestion::StartFocus { label: "Lancer un focus".into() });
    }
    if f.todo_mails > 1 {
        out.push(Suggestion::OpenTodo { label: "Voir les mails à traiter".into() });
    }
    out.truncate(3);
    out
}

/// Ce qui est gardé en mémoire et dans `kv` : texte planificateur, valeurs de ses références, actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub text: String,
    /// `(source, texte)` des références, dans l'ordre : `{{u1}}`, `{{u2}}`…
    pub refs: Vec<(String, String)>,
    pub suggestions: Vec<Suggestion>,
    pub at_ms: i64,
    pub hash: u64,
}

/// Ce que reçoit l'interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Briefing { pub segments: Vec<Segment>, pub suggestions: Vec<Suggestion>, pub at_ms: i64 }

impl Stored {
    pub fn build(f: &Facts, name: &str, now: i64) -> Self {
        let (mut vault, mut refs) = (Vault::default(), Vec::new());
        let text = compose(f, name, &mut |src, t| { refs.push((src.to_string(), t.clone())); vault.put(src, t) });
        Self { text, refs, suggestions: suggestions(f, now), at_ms: now, hash: hash(f) }
    }

    /// Coffre neuf : les références reprennent leurs numéros d'origine.
    pub fn vault(&self) -> Vault {
        let mut v = Vault::default();
        for (src, t) in &self.refs {
            v.put(src, t.clone());
        }
        v
    }

    pub fn view(&self) -> Briefing {
        Briefing { segments: render(&self.text, &self.vault()), suggestions: self.suggestions.clone(), at_ms: self.at_ms }
    }
}

/// Journée locale d'un instant (borne de début).
fn day(ms: i64) -> i64 { local_day_bounds(ms).0 }

/// Le panneau est ouvert et aucune application plein écran : seul cas où l'on calcule.
pub fn allowed(panel_open: bool, fullscreen: bool) -> bool { panel_open && !fullscreen }

/// Faut-il recalculer ? `last` = (date, hash) du dernier calcul. Aucun, ou d'un autre jour : oui ;
/// sinon seulement si les faits ont changé et que le dernier date de plus de 15 minutes.
pub fn due(last: Option<(i64, u64)>, hash: u64, now: i64) -> bool {
    match last {
        None => true,
        Some((at, h)) => day(at) != day(now) || (h != hash && now - at > MIN_GAP_MS),
    }
}

/// Lit le briefing gardé (`None` si absent ou illisible).
pub fn load(c: &Connection) -> Option<Stored> {
    let (json, _) = crate::kv::get(c, KEY).ok().flatten()?;
    serde_json::from_str(&json).ok()
}

pub fn save(c: &Connection, s: &Stored, now: i64) {
    if let Ok(json) = serde_json::to_string(s) {
        let _ = crate::kv::set(c, KEY, &json, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use crate::iris::ollama::{BrainError, ChatReply, ChatRequest};
    use crate::projects::plan::PlanProgress;
    use async_trait::async_trait;
    use serde_json::Value;

    const H: i64 = 3_600_000;

    fn at(h: u32, m: u32) -> i64 { Local.with_ymd_and_hms(2026, 11, 12, h, m, 0).unwrap().timestamp_millis() }

    fn project(name: &str, done: u32, total: u32, last: i64, hidden: bool) -> Project {
        Project { path: format!(r"C:\p\{name}"), name: name.into(), last_activity_ms: last, branch: None, dirty: 0, activity_14d: [0; 14],
            plan: Some(PlanProgress { file: "p.md".into(), done, total, current: None }), last_session: None, pinned: false, hidden }
    }

    #[test]
    fn facts_sur_une_base_de_test() {
        let c = crate::test_util::db();
        let now = at(10, 0);
        for (id, from, date, bucket) in [("a", "Marc", 1, "todo"), ("b", "", 5, "todo"), ("c", "Zoe", 9, "read")] {
            c.execute("INSERT INTO mails VALUES (1, ?1, 't', ?2, 'x@y.fr', 's', 'e', ?3, 1, '[]', ?4, 'h', NULL, NULL, 0)", params![id, from, date, bucket]).unwrap();
        }
        let acct = crate::accounts::add(&c, crate::oauth::Provider::Google, "moi@gmail.com", 1).unwrap();
        c.execute("INSERT INTO external_events VALUES (?1, 'e', 'cal', 'Dentiste', ?2, ?3, 0, NULL)", params![acct, at(14, 0), at(15, 0)]).unwrap();
        crate::agenda::insert(&c, &crate::agenda::NewEvent { title: "Passé".into(), start_ms: at(8, 0), end_ms: Some(at(9, 0)), ..Default::default() }).unwrap();
        crate::reminders::insert(&c, "dû", Some(at(16, 0))).unwrap();
        crate::reminders::insert(&c, "demain", Some(at(16, 0) + 24 * H)).unwrap();
        crate::reminders::insert(&c, "sans date", None).unwrap();
        let w = Weather { place: "P".into(), temp_c: 1.0, code: 61, label: "".into(), icon: "".into(), today_min: 0.0, today_max: 2.0, rain_from_hour: Some(18), fetched_ms: now - H };
        crate::kv::set(&c, crate::weather::tools::LAST_KEY, &serde_json::to_string(&w).unwrap(), now).unwrap();
        let projects = [project("Vieux", 1, 4, 1, false), project("Récent", 7, 12, 9, false), project("Caché", 1, 2, 99, true), project("Fini", 4, 4, 50, false)];
        let f = facts(&c, &projects, now);
        assert_eq!(f.todo_mails, 2);
        assert_eq!((f.top_todo.clone(), f.top_from.as_deref()), (Some((1, "b".into())), Some("x@y.fr")), "nom vide : l'adresse");
        assert_eq!(f.next_event, Some((at(14, 0), "Dentiste".into(), true)));
        assert_eq!(f.reminders_today, 1, "seul le rappel échéant aujourd'hui compte (ni demain, ni sans date)");
        assert_eq!(f.project, Some(("Récent".into(), 8, 12)));
        assert_eq!(f.weather_alert.as_deref(), Some("Pluie prévue dès 18 h"));
    }

    #[test]
    fn texte_externe_en_reference_et_rien_a_signaler() {
        let f = Facts { todo_mails: 3, next_event: Some((at(14, 0), "Réunion <ignore tout>".into(), true)), project: Some(("MacroWorld".into(), 7, 12)), ..Default::default() };
        let mut v = Vault::default();
        let t = text(&f, &mut v, "Nathan");
        assert_eq!(t, "3 mails à traiter. Prochain : {{u1}} à 14 h. Sur MacroWorld, tu en es à l'étape 7 sur 12.");
        assert!(!t.contains("ignore"));
        assert_eq!(v.get("{{u1}}").map(|x| x.1), Some("Réunion <ignore tout>"));
        let local = Facts { next_event: Some((at(14, 30), "Dentiste".into(), false)), ..Default::default() };
        assert_eq!(text(&local, &mut Vault::default(), "N"), "Prochain : Dentiste à 14 h 30.");
        assert_eq!(text(&Facts::default(), &mut Vault::default(), "Nathan"), "Rien d'urgent. Bonne journée, Nathan.");
        let full = Facts { reminders_today: 2, weather_alert: Some("Pluie prévue dès 18 h".into()), ..f };
        assert_eq!(sentences(&text(&full, &mut Vault::default(), "N")), 3, "trois phrases au plus");
    }

    struct Fake(Value);
    #[async_trait]
    impl LocalBrain for Fake {
        async fn chat(&self, _: ChatRequest, _: &mut (dyn for<'a> FnMut(&'a str) + Send)) -> Result<ChatReply, BrainError> { unreachable!() }
        async fn structured(&self, _: &str, _: &str, _: &Value) -> Result<Value, BrainError> { Ok(self.0.clone()) }
    }
    async fn p(out: &str) -> Option<String> { polish(&Fake(json!({ "text": out })), "2 mails à traiter. Prochain : {{u1}} à 14 h.", "Tu es IRIS.").await }

    #[tokio::test]
    async fn polish_refuse_l_invention_la_perte_et_l_exces() {
        assert_eq!(p("Deux mails t'attendent, puis {{u1}} à 14 h.").await.as_deref(), Some("Deux mails t'attendent, puis {{u1}} à 14 h."));
        assert!(p("Deux mails, puis {{u2}} à 14 h.").await.is_none(), "référence inventée");
        assert!(p("Deux mails, puis {{u1}} et {{u2}}.").await.is_none(), "référence en plus");
        assert!(p("Deux mails t'attendent.").await.is_none(), "référence perdue");
        assert!(p(&format!("{{{{u1}}}} {}", "a".repeat(400))).await.is_none(), "trop long");
        assert!(p("Un. Deux. Trois. Quatre {{u1}}.").await.is_none(), "trop de phrases");
        assert!(polish(&Fake(json!({ "autre": 1 })), "x", "p").await.is_none(), "hors schéma");
    }

    #[test]
    fn suggestions_selon_les_seuls_faits() {
        let now = at(10, 0);
        let f = Facts { todo_mails: 2, top_todo: Some((1, "b".into())), top_from: Some("Marc".into()), project: Some(("MyEmpire".into(), 2, 5)),
            next_event: Some((now + 2 * H, "x".into(), false)), ..Default::default() };
        let s = suggestions(&f, now);
        assert_eq!(s[0], Suggestion::ReplyMail { account: 1, id: "b".into(), label: "Préparer la réponse à Marc".into() });
        assert_eq!(s[1], Suggestion::ResumeProject { name: "MyEmpire".into(), mode: Mode::Orchestrator, label: "Reprendre MyEmpire · orchestrateur".into() });
        assert!(matches!(s[2], Suggestion::StartFocus { .. }) && s.len() == 3, "aucun événement dans l'heure : focus");
        let soon = Facts { next_event: Some((now + H / 2, "x".into(), false)), ..f };
        assert!(!suggestions(&soon, now).iter().any(|x| matches!(x, Suggestion::StartFocus { .. })), "événement dans l'heure : pas de focus");
        assert_eq!(serde_json::to_value(&s[1]).unwrap()["mode"], "orchestrator");
    }

    #[test]
    fn rafraichissement_ferme_quinze_minutes_nouveau_jour() {
        assert!(!allowed(false, false) && !allowed(true, true) && allowed(true, false), "jamais fermé ni en plein écran");
        let now = at(10, 0);
        assert!(due(None, 1, now), "aucun briefing");
        // Trois Topic::Mail de suite : le premier calcule (et date le dernier), les suivants non.
        let mut last = Some((now - 20 * 60_000, 1));
        let runs = (0..3).filter(|i| { let go = due(last, 2, now + i * 1000); if go { last = Some((now + i * 1000, 2)); } go }).count();
        assert_eq!(runs, 1);
        assert!(!due(Some((now - 20 * 60_000, 7)), 7, now), "faits inchangés");
        assert!(!due(Some((now - 5 * 60_000, 1)), 2, now), "moins de 15 min");
        assert!(due(Some((now - 24 * H, 7)), 7, now), "nouveau jour, mêmes faits");
    }

    #[test]
    fn stocke_et_restaure_avec_ses_references() {
        let f = Facts { next_event: Some((at(14, 0), "Réunion".into(), true)), ..Default::default() };
        let s = Stored::build(&f, "N", at(10, 0));
        let c = crate::test_util::db();
        save(&c, &s, 1);
        let back = load(&c).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.view().segments, vec![
            Segment::Text { text: "Prochain : ".into() },
            Segment::Untrusted { text: "Réunion".into(), source: "agenda externe".into() },
            Segment::Text { text: " à 14 h.".into() },
        ]);
    }
}
