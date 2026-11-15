//! Briefing d'IRIS dans l'en-tête du panneau : l'ancien s'affiche tout de suite (mémoire, jamais la base), le
//! nouveau se calcule en tâche de fond, panneau ouvert et hors plein écran seulement. À la première ouverture de la
//! journée, IRIS peut aussi le dire à voix haute (`voice_briefing_auto`). Journal : aucun contenu.
use crate::commands::dispatch_as_user;
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::iris::briefing::{self, Briefing, Stored, Suggestion};
use oniris_core::iris::{LocalBrain, LocalStatus, Ollama, Segment};
use oniris_core::time_util::now_ms;
use oniris_core::tools::ToolOutcome;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};

/// Dernier briefing (mémoire) ; restauré de `kv` au démarrage.
static CURRENT: Mutex<Option<Stored>> = parking_lot::const_mutex(None);

/// À appeler dans `setup` : reprend le briefing gardé pour que la première ouverture affiche déjà quelque chose.
pub fn load(app: &AppHandle) {
    *CURRENT.lock() = briefing::load(&app.state::<AppState>().db.lock());
}

/// Appelé par `windows::open_panel` (fil principal) : ne verrouille pas la base.
pub fn on_panel_open(app: &AppHandle) {
    spawn_refresh(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move { say_on_open(&app) });
}

/// Briefing dit une fois par jour à l'ouverture du panneau ; le jour n'est retenu que s'il a vraiment été dit.
fn say_on_open(app: &AppHandle) {
    // Deux ouvertures rapprochées : une seule décision à la fois.
    static ONE: Mutex<()> = parking_lot::const_mutex(());
    let _one = ONE.lock();
    let st = app.state::<AppState>();
    let auto = st.settings.read().voice_briefing_auto;
    let now = now_ms();
    let last = oniris_core::kv::get(&st.db.lock(), briefing::SPOKEN_KEY).ok().flatten().and_then(|(v, _)| v.parse().ok());
    if !briefing::speak_on_open(auto, crate::fullscreen::active(), last, now) {
        return;
    }
    if crate::modules::voice::pipeline::speak_briefing(&spoken(app, true)) {
        tracing::info!("briefing dit à voix haute");
        let _ = oniris_core::kv::set(&st.db.lock(), briefing::SPOKEN_KEY, &briefing::spoken_day(now).to_string(), now);
    }
}

/// Briefing à dire : celui du panneau s'il est du jour et à jour (reformulé par IRIS), sinon recalculé ; `greet` :
/// une salutation devant (briefing du matin).
pub fn spoken(app: &AppHandle, greet: bool) -> Vec<Segment> {
    let st = app.state::<AppState>();
    let now = now_ms();
    let projects = st.projects.read().list.clone();
    let facts = briefing::facts(&st.db.lock(), &projects, now);
    let name = st.settings.read().user_name.clone();
    let fresh = CURRENT.lock().clone().filter(|c| !briefing::due(Some((c.at_ms, c.hash)), briefing::hash(&facts), now) && c.hash == briefing::hash(&facts));
    let mut segments = fresh.unwrap_or_else(|| Stored::build(&facts, &name, now)).view().segments;
    if greet {
        let hello = if name.is_empty() { "Bonjour ! ".to_string() } else { format!("Bonjour {name} ! ") };
        segments.insert(0, Segment::Text { text: hello });
    }
    segments
}

/// `Topic::Mail` ou `Topic::Agenda` changé (appelé par `TauriEvents`).
pub fn on_topic(app: &AppHandle, topic: Topic) {
    if matches!(topic, Topic::Mail | Topic::Agenda) {
        spawn_refresh(app);
    }
}

fn gate(app: &AppHandle) -> bool {
    briefing::allowed(app.state::<AppState>().panel_open.load(Ordering::SeqCst), crate::fullscreen::active())
}

fn spawn_refresh(app: &AppHandle) {
    if !gate(app) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { refresh(&app).await });
}

async fn refresh(app: &AppHandle) {
    let st = app.state::<AppState>();
    let now = now_ms();
    let projects = st.projects.read().list.clone();
    let facts = briefing::facts(&st.db.lock(), &projects, now);
    let (name, personality, model, keep) = {
        let s = st.settings.read();
        (s.user_name.clone(), s.iris_personality.replace("{prénom}", &s.user_name), s.iris_local_model.clone(), s.iris_unload_minutes)
    };
    let stored = {
        // Décision et enregistrement d'un seul tenant : trois événements de suite ne donnent qu'un calcul.
        let mut cur = CURRENT.lock();
        if !briefing::due(cur.as_ref().map(|s| (s.at_ms, s.hash)), briefing::hash(&facts), now) {
            return;
        }
        let s = Stored::build(&facts, &name, now);
        *cur = Some(s.clone());
        s
    };
    briefing::save(&st.db.lock(), &stored, now);
    st.events.changed(Topic::Iris);

    let ollama = Ollama::new(&model, keep);
    if ollama.status(&model).await != LocalStatus::Ready || !gate(app) {
        return;
    }
    let polished = briefing::polish(&ollama as &dyn LocalBrain, &stored.text, &personality).await;
    crate::modules::iris::unload_if_fullscreen(app).await;
    let Some(polished) = polished else { return };
    let updated = {
        let mut cur = CURRENT.lock();
        match cur.as_mut().filter(|c| c.at_ms == stored.at_ms) {
            Some(c) => { c.text = polished; c.clone() }
            None => return, // remplacé entre-temps
        }
    };
    briefing::save(&st.db.lock(), &updated, now_ms());
    st.events.changed(Topic::Iris);
}

/// Le briefing en mémoire : aucune base, aucun calcul.
#[tauri::command]
pub async fn iris_briefing() -> Option<Briefing> {
    CURRENT.lock().as_ref().map(Stored::view)
}

/// Exécute l'action proposée `index` (choisie par le code, jamais par un modèle) du briefing `at_ms` :
/// un briefing remplacé entre-temps refuse, l'index viserait une autre action.
#[tauri::command]
pub async fn iris_briefing_act(app: AppHandle, index: usize, at_ms: i64) -> Result<(), String> {
    let s = {
        let cur = CURRENT.lock();
        let b = cur.as_ref().filter(|b| b.at_ms == at_ms).ok_or("briefing_stale")?;
        b.suggestions.get(index).cloned().ok_or("action_unknown")?
    };
    let outcome = match s {
        Suggestion::ReplyMail { account, id, .. } => return open_mail(&app, Some((account, id)), true),
        Suggestion::OpenTodo { .. } => return open_mail(&app, None, false),
        Suggestion::ResumeProject { name, mode, .. } => dispatch_as_user(&app, "projects.resume", json!({ "name": name, "mode": mode })).await,
        Suggestion::StartFocus { .. } => dispatch_as_user(&app, "focus.start", json!({})).await,
    };
    match outcome {
        ToolOutcome::Done { .. } | ToolOutcome::NeedsConfirmation(_) => Ok(()),
        _ => Err("action_failed".into()),
    }
}

/// La carte Mails vient au premier plan ; avec `draft`, elle ouvre le mail et lance « Répondre avec IRIS ».
fn open_mail(app: &AppHandle, mail: Option<(i64, String)>, draft: bool) -> Result<(), String> {
    let (account, id) = mail.unzip();
    app.emit_to("panel", "oniris://iris-open-mail", json!({ "account": account, "id": id, "draft": draft })).map_err(|e| e.to_string())
}
