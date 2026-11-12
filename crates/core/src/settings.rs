use crate::CoreError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// `key` : "Space", "A".."Z" ou "F1".."F12".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySetting {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String,
}

impl Default for HotkeySetting {
    fn default() -> Self {
        Self { alt: true, ctrl: false, shift: false, win: false, key: "Space".into() }
    }
}

/// Site surveillé : `url` en http(s), `slow_ms` : seuil de lenteur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub name: String,
    pub url: String,
    #[serde(default = "default_slow_ms")]
    pub slow_ms: u32,
}

fn default_slow_ms() -> u32 { 1500 }

const MAX_SITES: usize = 30;

fn same_site(a: &str, b: &url::Url) -> bool {
    url::Url::parse(a).is_ok_and(|a| a.as_str().trim_end_matches('/') == b.as_str().trim_end_matches('/'))
}

/// Rogne, puis : nom non vide (40), URL http(s) avec hôte (300), seuil 300..=10000, pas de doublon d'URL.
fn check_sites(sites: &mut [Site]) -> Result<(), String> {
    for s in sites.iter_mut() {
        s.name = s.name.trim().to_string();
        s.url = s.url.trim().to_string();
    }
    if sites.len() > MAX_SITES {
        return Err(format!("sites : {MAX_SITES} au plus"));
    }
    for (i, s) in sites.iter().enumerate() {
        // Pas d'identifiants dans l'adresse (user:pw@) : ils finiraient en clair dans la base et les réglages.
        let http = url::Url::parse(&s.url).ok().filter(|u| {
            matches!(u.scheme(), "http" | "https") && u.host_str().is_some() && u.username().is_empty() && u.password().is_none()
        });
        let Some(http) = http.filter(|_| s.url.chars().count() <= 300) else {
            return Err("sites : adresse http(s) de 300 caractères au plus, sans identifiants".into());
        };
        if s.name.is_empty() || s.name.chars().count() > 40 {
            return Err("sites : nom de 1 à 40 caractères".into());
        }
        if !(300..=10_000).contains(&s.slow_ms) {
            return Err("sites : seuil de lenteur hors de 300..=10000 ms".into());
        }
        // Doublon : même adresse à la casse de l'hôte et à la barre finale près.
        if sites[..i].iter().any(|o| same_site(&o.url, &http)) {
            return Err("sites : adresse déjà surveillée".into());
        }
    }
    Ok(())
}

/// Ajoute `https://<domaine>` pour chaque domaine absent, par la même validation que les réglages
/// (doublons via `same_site`, plafond de 30). Renvoie le nombre de sites ajoutés.
pub fn merge_domains(sites: &mut Vec<Site>, domains: &[String]) -> usize {
    let before = sites.len();
    for d in domains {
        if sites.len() >= MAX_SITES {
            break;
        }
        let name: String = d.chars().take(40).collect();
        sites.push(Site { name, url: format!("https://{d}"), slow_ms: default_slow_ms() });
        if check_sites(sites).is_err() {
            sites.pop();
        }
    }
    sites.len() - before
}

/// Chemin d'un exécutable (Zenytt, Claude) : vide (détection automatique) ou absolu, en `.exe`, 260 caractères au plus,
/// jamais réseau (UNC `\\srv\…`) ni préfixé `\\?\`.
fn check_exe_path(p: &str) -> Result<(), String> {
    if p.is_empty() || (p.chars().count() <= 260 && !p.starts_with(r"\\") && !p.starts_with("//") && std::path::Path::new(p).is_absolute() && p.to_ascii_lowercase().ends_with(".exe")) {
        Ok(())
    } else {
        Err("chemin de l'exécutable : absolu, en .exe, 260 caractères au plus".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub user_name: String,
    pub hotkey: HotkeySetting,
    pub hold_threshold_ms: u32,
    pub hover_open: bool,
    pub hover_delay_ms: u32,
    pub autostart: bool,
    pub hide_pill_in_fullscreen: bool,
    pub timeline_start_hour: u8,
    pub timeline_end_hour: u8,
    pub reduce_motion: bool,
    pub focus_minutes: u16,
    pub break_minutes: u16,
    pub clipboard_enabled: bool,
    pub clipboard_retention_days: u16,
    pub clipboard_max_items: u16,
    /// Ville de la météo : vide = météo désactivée (aucune requête). 80 caractères au plus.
    pub weather_city: String,
    /// Dossiers indexés par la recherche : vide = dossier utilisateur. 20 chemins absolus au plus (260 caractères).
    pub search_roots: Vec<String>,
    /// Dossiers où chercher des projets, indexés en plus de `search_roots`. Mêmes limites.
    pub projects_roots: Vec<String>,
    /// Invites de « Reprendre » (Claude Code), non vides, 2000 caractères au plus.
    pub resume_prompt_normal: String,
    pub resume_prompt_orchestrator: String,
    /// Identifiants client OAuth (vide = non configuré) : 200 caractères au plus, sans espace.
    pub google_client_id: String,
    pub spotify_client_id: String,
    /// Intervalle de relève des mails, 2..=5 minutes.
    pub mail_poll_minutes: u8,
    /// Sites surveillés par sondes toutes les 60 s (30 au plus).
    pub sites: Vec<Site>,
    /// Chemin de Zenytt.exe (serveur MCP de la charge du VPS) : vide = détection automatique.
    pub zenytt_path: String,
    /// Chemin de claude.exe : vide = détection automatique.
    pub claude_path: String,
    /// Modèle de Claude Code (`--model`) : vide = celui par défaut ; commence par un alphanumérique ; 100 caractères au plus parmi `[A-Za-z0-9._:/-]` et crochets.
    pub iris_claude_model: String,
    /// Commandes PowerShell d'IRIS exécutées sans carte (préfixes de jetons) : 50 au plus, ASCII, 100 caractères.
    pub iris_shell_allowlist: Vec<String>,
    /// Dossiers de travail d'IRIS : renommer, déplacer ou supprimer dedans se fait sans carte. 20 au plus, absolus,
    /// locaux, jamais protégés.
    pub iris_work_dirs: Vec<String>,
    /// Modèle Ollama du cerveau local : 1 à 100 caractères parmi `[a-z0-9._:/-]`.
    pub iris_local_model: String,
    /// IRIS peut confier une demande à Claude d'elle-même (délégation, local indisponible, résumé long).
    /// Faux : Claude seulement sur demande explicite (bouton, « demande à Claude »), sinon elle le propose.
    pub iris_claude_auto: bool,
    /// Minutes avant que le modèle local quitte la mémoire, 1..=60.
    pub iris_unload_minutes: u8,
    /// Personnalité d'IRIS (`{prénom}` est remplacé) : non vide, 2000 caractères au plus.
    pub iris_personality: String,
    /// Modèle de transcription : `small` (rapide) ou `turbo` (meilleur, plus lourd).
    pub voice_stt_model: String,
    /// IRIS répond à voix haute.
    pub voice_tts: bool,
    /// Volume de la voix, 0..=100.
    pub voice_volume: u8,
    /// Minutes d'inactivité avant l'arrêt de Whisper, 1..=30.
    pub voice_unload_minutes: u8,
    /// Nom du micro : vide = périphérique par défaut ; 200 caractères au plus.
    pub voice_input_device: String,
    /// Mot d'activation « Dis Iris » : écoute continue, désactivé par défaut.
    pub voice_wake: bool,
    /// Mode apprentissage du mot : extraits étiquetés écrits sur le disque, désactivé par défaut.
    pub voice_wake_learning: bool,
}

/// Commandes en lecture seule (aucun de leurs arguments sûrs n'écrit).
const DEFAULT_SHELL_ALLOWLIST: [&str; 4] = ["git status", "Get-ChildItem", "Get-Location", "Get-Date"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_name: String::new(),
            hotkey: HotkeySetting::default(),
            hold_threshold_ms: 250,
            hover_open: true,
            hover_delay_ms: 350,
            autostart: true,
            hide_pill_in_fullscreen: true,
            timeline_start_hour: 8,
            timeline_end_hour: 23,
            reduce_motion: false,
            focus_minutes: 25,
            break_minutes: 5,
            clipboard_enabled: true,
            clipboard_retention_days: 7,
            clipboard_max_items: 200,
            weather_city: String::new(),
            search_roots: Vec::new(),
            projects_roots: Vec::new(),
            resume_prompt_normal: RESUME_NORMAL.into(),
            resume_prompt_orchestrator: format!("{RESUME_NORMAL} {RESUME_ORCHESTRATOR}"),
            google_client_id: String::new(),
            spotify_client_id: String::new(),
            mail_poll_minutes: 3,
            sites: Vec::new(),
            zenytt_path: String::new(),
            claude_path: String::new(),
            iris_claude_model: String::new(),
            iris_shell_allowlist: DEFAULT_SHELL_ALLOWLIST.map(String::from).to_vec(),
            iris_work_dirs: Vec::new(),
            iris_local_model: "qwen3:14b".into(),
            iris_claude_auto: false,
            iris_unload_minutes: 30,
            iris_personality: crate::iris::prompt::PERSONALITY.into(),
            voice_stt_model: "small".into(),
            voice_tts: true,
            voice_volume: 80,
            voice_unload_minutes: 3,
            voice_input_device: String::new(),
            voice_wake: false,
            voice_wake_learning: false,
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

fn check_hotkey(h: &HotkeySetting) -> Result<(), String> {
    let vk = key_to_vk(&h.key).ok_or_else(|| format!("touche inconnue : {}", h.key))?;
    // Sans Alt/Ctrl/Win, le crochet avalerait la touche à chaque frappe ordinaire : F1-F12 seulement.
    if !(h.alt || h.ctrl || h.win) && !(0x70..=0x7B).contains(&vk) {
        return Err("raccourci sans Alt, Ctrl ni Win : réservé aux touches F1 à F12".into());
    }
    Ok(())
}

const RESUME_NORMAL: &str =
    "Reprends là où tu en étais sur ce projet. Appuie-toi sur le plan de développement et l'historique git. Commence par me dire où on en est.";
const RESUME_ORCHESTRATOR: &str = "Travaille en mode orchestrateur, comme décrit dans mon CLAUDE.md global : graphify pour t'orienter, sous-agents haiku/sonnet/opus selon la tâche, parallélisation des tâches indépendantes, et relecture avant intégration.";
const MAX_PROMPT: usize = 2000;

/// Non vide, 2000 caractères au plus, et un lien `vscode://` qui tient dans la limite une fois encodé.
fn check_prompt(p: &str) -> Result<(), String> {
    use crate::projects::resume::{vscode_uri, DUMMY_UUID};
    if p.trim().is_empty() || p.chars().count() > MAX_PROMPT {
        return Err(format!("non vide, {MAX_PROMPT} caractères au plus"));
    }
    if vscode_uri(p, Some(DUMMY_UUID)).len() > crate::launch::MAX_URL {
        return Err("Consigne trop longue une fois encodée (max ~600 caractères selon les accents)".into());
    }
    Ok(())
}

const MAX_ROOTS: usize = 20;
const MAX_ROOT_LEN: usize = 260;

/// Valide une liste de dossiers (déjà normalisée) ; l'erreur nomme le champ.
fn check_roots(roots: &[String], field: &str) -> Result<(), String> {
    if roots.len() > MAX_ROOTS {
        return Err(format!("{field} : {MAX_ROOTS} dossiers au plus"));
    }
    if roots.iter().any(|r| r.chars().count() > MAX_ROOT_LEN || !std::path::Path::new(r).is_absolute()) {
        return Err(format!("{field} : chemins absolus de {MAX_ROOT_LEN} caractères au plus"));
    }
    Ok(())
}

/// Racines de projets : elles élargissent la liste blanche de `files.open`/`files.run`, donc ni racine de lecteur ni `..`.
fn check_project_roots(roots: &[String]) -> Result<(), String> {
    check_roots(roots, "dossiers de projets")?;
    for r in roots {
        let p = std::path::Path::new(r);
        if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err("dossiers de projets : '..' interdit".into());
        }
        if p.parent().is_none() {
            return Err("dossiers de projets : choisis un dossier précis, pas un lecteur entier".into());
        }
    }
    Ok(())
}

/// Dossiers de travail : mêmes garde-fous que l'écriture d'IRIS (réseau refusé avant tout accès disque).
fn check_work_dirs(dirs: &[String]) -> Result<(), String> {
    check_roots(dirs, "dossiers de travail")?;
    let prot = crate::general::files::protected_dirs();
    for d in dirs {
        crate::general::files::work_dir_ok(std::path::Path::new(d), &prot).map_err(|e| format!("dossiers de travail : {e}"))?;
    }
    Ok(())
}

impl Settings {
    /// Rogne les dossiers, retire les vides et les doublons (casse ignorée, comme Windows). Sans erreur possible.
    pub fn normalize_roots(&mut self) {
        for list in [&mut self.search_roots, &mut self.projects_roots, &mut self.iris_work_dirs] {
            let mut seen: Vec<String> = Vec::new();
            for r in list.drain(..) {
                let r = r.trim().to_string();
                if !r.is_empty() && !seen.iter().any(|s| s.eq_ignore_ascii_case(&r)) {
                    seen.push(r);
                }
            }
            *list = seen;
        }
    }

    /// Remet à son défaut chaque champ invalide (les heures de la frise vont ensemble) ; renvoie les erreurs.
    fn fix(&mut self) -> Vec<String> {
        let d = Settings::default();
        let mut errs = Vec::new();
        if self.timeline_start_hour >= self.timeline_end_hour || self.timeline_end_hour > 24 {
            errs.push("plage horaire invalide : début < fin <= 24".to_string());
            (self.timeline_start_hour, self.timeline_end_hour) = (d.timeline_start_hour, d.timeline_end_hour);
        }
        if !(100..=1000).contains(&self.hold_threshold_ms) {
            errs.push("seuil de maintien hors de 100..=1000 ms".into());
            self.hold_threshold_ms = d.hold_threshold_ms;
        }
        if !(100..=3000).contains(&self.hover_delay_ms) {
            errs.push("délai de survol hors de 100..=3000 ms".into());
            self.hover_delay_ms = d.hover_delay_ms;
        }
        if !(5..=120).contains(&self.focus_minutes) {
            errs.push("durée de focus hors de 5..=120 min".into());
            self.focus_minutes = d.focus_minutes;
        }
        if !(1..=60).contains(&self.break_minutes) {
            errs.push("durée de pause hors de 1..=60 min".into());
            self.break_minutes = d.break_minutes;
        }
        if !(1..=90).contains(&self.clipboard_retention_days) {
            errs.push("conservation du presse-papiers hors de 1..=90 jours".into());
            self.clipboard_retention_days = d.clipboard_retention_days;
        }
        if !(20..=1000).contains(&self.clipboard_max_items) {
            errs.push("taille de l'historique du presse-papiers hors de 20..=1000".into());
            self.clipboard_max_items = d.clipboard_max_items;
        }
        if !(2..=5).contains(&self.mail_poll_minutes) {
            errs.push("relève des mails hors de 2..=5 min".into());
            self.mail_poll_minutes = d.mail_poll_minutes;
        }
        self.weather_city = self.weather_city.trim().to_string();
        if self.weather_city.chars().count() > 80 {
            errs.push("ville de la météo : 80 caractères au plus".into());
            self.weather_city = d.weather_city.clone();
        }
        self.normalize_roots();
        // Peut viser un lecteur entier : élargit la liste blanche d'ouverture par choix.
        if let Err(e) = check_roots(&self.search_roots, "dossiers de recherche") {
            errs.push(e);
            self.search_roots = d.search_roots.clone();
        }
        if let Err(e) = check_project_roots(&self.projects_roots) {
            errs.push(e);
            self.projects_roots = d.projects_roots.clone();
        }
        if let Err(e) = check_prompt(&self.resume_prompt_normal) {
            errs.push(format!("consigne de reprise : {e}"));
            self.resume_prompt_normal = d.resume_prompt_normal.clone();
        }
        if let Err(e) = check_prompt(&self.resume_prompt_orchestrator) {
            errs.push(format!("consigne de reprise (orchestrateur) : {e}"));
            self.resume_prompt_orchestrator = d.resume_prompt_orchestrator.clone();
        }
        for (id, def, field) in [
            (&mut self.google_client_id, &d.google_client_id, "identifiant client Google"),
            (&mut self.spotify_client_id, &d.spotify_client_id, "identifiant client Spotify"),
        ] {
            *id = id.trim().to_string();
            if id.chars().count() > 200 || id.chars().any(char::is_whitespace) {
                errs.push(format!("{field} : 200 caractères au plus, sans espace"));
                *id = def.clone();
            }
        }
        if let Err(e) = check_sites(&mut self.sites) {
            errs.push(e);
            self.sites = d.sites.clone();
        }
        self.zenytt_path = self.zenytt_path.trim().to_string();
        if let Err(e) = check_exe_path(&self.zenytt_path) {
            errs.push(e);
            self.zenytt_path = d.zenytt_path.clone();
        }
        self.claude_path = self.claude_path.trim().to_string();
        if let Err(e) = check_exe_path(&self.claude_path) {
            errs.push(e);
            self.claude_path = d.claude_path.clone();
        }
        self.iris_claude_model = self.iris_claude_model.trim().to_string();
        if self.iris_claude_model.len() > 100 || self.iris_claude_model.starts_with(|c: char| !c.is_ascii_alphanumeric()) || !self.iris_claude_model.bytes().all(|b| b.is_ascii_alphanumeric() || b".:_/-[]".contains(&b)) {
            errs.push("modèle Claude : 100 caractères au plus parmi A-Z a-z 0-9 . _ : / - [ ]".into());
            self.iris_claude_model = d.iris_claude_model.clone();
        }
        for e in &mut self.iris_shell_allowlist {
            *e = e.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        if self.iris_shell_allowlist.len() > 50
            || self.iris_shell_allowlist.iter().any(|e| e.is_empty() || e.len() > 100 || !crate::general::shell::allowlisted(e, std::slice::from_ref(e)))
        {
            errs.push("commandes sans confirmation : 50 au plus, ASCII sans caractère de contrôle du shell".into());
            self.iris_shell_allowlist = d.iris_shell_allowlist.clone();
        }
        if let Err(e) = check_work_dirs(&self.iris_work_dirs) {
            errs.push(e);
            self.iris_work_dirs = d.iris_work_dirs.clone();
        }
        let model_ok = !self.iris_local_model.is_empty()
            && self.iris_local_model.len() <= 100
            && self.iris_local_model.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b':' | b'/' | b'-'));
        if !model_ok {
            errs.push("modèle local : 1 à 100 caractères parmi a-z 0-9 . _ : / -".into());
            self.iris_local_model = d.iris_local_model.clone();
        }
        if !(1..=60).contains(&self.iris_unload_minutes) {
            errs.push("déchargement du modèle : 1 à 60 minutes".into());
            self.iris_unload_minutes = d.iris_unload_minutes;
        }
        self.iris_personality = self.iris_personality.trim().to_string();
        if self.iris_personality.is_empty() || self.iris_personality.chars().count() > 2000 {
            errs.push("personnalité d'IRIS : 1 à 2000 caractères".into());
            self.iris_personality = d.iris_personality.clone();
        }
        if !matches!(self.voice_stt_model.as_str(), "small" | "turbo") {
            errs.push("modèle de transcription : small ou turbo".into());
            self.voice_stt_model = d.voice_stt_model.clone();
        }
        if self.voice_volume > 100 {
            errs.push("volume de la voix : 0 à 100".into());
            self.voice_volume = d.voice_volume;
        }
        if !(1..=30).contains(&self.voice_unload_minutes) {
            errs.push("arrêt de la transcription : 1 à 30 minutes".into());
            self.voice_unload_minutes = d.voice_unload_minutes;
        }
        self.voice_input_device = self.voice_input_device.trim().to_string();
        if self.voice_input_device.chars().count() > 200 {
            errs.push("micro : 200 caractères au plus".into());
            self.voice_input_device = d.voice_input_device.clone();
        }
        if let Err(e) = check_hotkey(&self.hotkey) {
            errs.push(e);
            self.hotkey = d.hotkey;
        }
        errs
    }

    pub fn validate(&self) -> Result<(), String> {
        let errs = self.clone().fix();
        if errs.is_empty() { Ok(()) } else { Err(errs.join(" ; ")) }
    }
}

/// Ligne absente : défauts insérés. JSON illisible : copié dans `broken_json`, remplacé par les défauts.
/// Champ invalide (même validation que l'enregistrement) : ce champ seul reprend son défaut.
pub fn load_settings(conn: &Connection) -> Result<Settings, CoreError> {
    let row: Option<String> =
        conn.query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0)).optional()?;
    match row {
        Some(json) => match serde_json::from_str::<Settings>(&json) {
            Ok(mut s) => {
                let errs = s.fix();
                if !errs.is_empty() {
                    tracing::warn!("réglages corrigés au chargement : {}", errs.join(" ; "));
                }
                Ok(s)
            }
            Err(_) => {
                let d = Settings::default();
                conn.execute(
                    "UPDATE settings SET broken_json = ?1, json = ?2 WHERE id = 1",
                    params![json, serde_json::to_string(&d)?],
                )?;
                Ok(d)
            }
        },
        None => {
            let d = Settings::default();
            save_settings(conn, &d)?;
            Ok(d)
        }
    }
}

pub fn save_settings(conn: &Connection, s: &Settings) -> Result<(), CoreError> {
    conn.execute(
        "INSERT INTO settings(id, json) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET json = excluded.json",
        params![serde_json::to_string(s)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrate, open_db, CORE_MIGRATIONS};
    #[test] fn champs_manquants_remplis_par_defaut() {
        let s: Settings = serde_json::from_str(r#"{"user_name":"Nathan"}"#).unwrap();
        assert_eq!(s.user_name, "Nathan");
        assert_eq!(s.hold_threshold_ms, 250);
        assert_eq!(s.timeline_end_hour, 23);
        assert_eq!((s.focus_minutes, s.break_minutes), (25, 5));
        assert_eq!((s.clipboard_enabled, s.clipboard_retention_days, s.clipboard_max_items), (true, 7, 200));
    }
    #[test] fn liste_blanche_shell_validee() {
        let with = |l: &[&str]| Settings { iris_shell_allowlist: l.iter().map(|x| x.to_string()).collect(), ..Settings::default() }.validate();
        assert!(with(&["git status", "npm run build"]).is_ok());
        assert!(with(&["git status; rm x"]).is_err());
        assert!(with(&[""]).is_err());
        assert!(with(&["é"]).is_err());
        assert!(with(&vec!["git status"; 51]).is_err());
    }
    #[test] fn modele_local_et_delai_de_dechargement_valides() {
        let m = |v: &str| Settings { iris_local_model: v.into(), ..Settings::default() }.validate();
        let u = |v: u8| Settings { iris_unload_minutes: v, ..Settings::default() }.validate();
        assert!(m("qwen3:8b").is_ok() && m("hf.co/a/b_c-1:q4").is_ok());
        assert!(m("QWEN 3").is_err() && m("").is_err() && m(&"a".repeat(101)).is_err());
        assert!(u(1).is_ok() && u(60).is_ok());
        assert!(u(0).is_err() && u(61).is_err());
    }
    #[test] fn voice_wake_defaults_to_false() { assert!(!Settings::default().voice_wake && !Settings::default().voice_wake_learning); }
    #[test] fn reglages_de_la_voix() {
        let d = Settings::default();
        assert_eq!((d.voice_stt_model.as_str(), d.voice_tts, d.voice_volume, d.voice_unload_minutes, d.voice_input_device.as_str()), ("small", true, 80, 3, ""));
        assert_eq!((d.iris_local_model.as_str(), d.iris_unload_minutes, d.iris_claude_auto), ("qwen3:14b", 30, false));
        let v = |f: fn(&mut Settings)| { let mut s = Settings::default(); f(&mut s); s.validate() };
        assert!(v(|s| s.voice_volume = 100).is_ok() && v(|s| s.voice_volume = 101).is_err());
        assert!(v(|s| s.voice_stt_model = "turbo".into()).is_ok() && v(|s| s.voice_stt_model = "medium".into()).is_err());
        assert!(v(|s| s.voice_unload_minutes = 30).is_ok() && v(|s| s.voice_unload_minutes = 0).is_err() && v(|s| s.voice_unload_minutes = 31).is_err());
        assert!(v(|s| s.voice_input_device = "é".repeat(200)).is_ok() && v(|s| s.voice_input_device = "a".repeat(201)).is_err());
    }
    #[test] fn personnalite_non_vide_et_bornee() {
        let p = |v: &str| Settings { iris_personality: v.into(), ..Settings::default() }.validate();
        assert!(p("Sois bref.").is_ok() && p(&"é".repeat(2000)).is_ok());
        assert!(p("").is_err() && p("  
 ").is_err() && p(&"a".repeat(2001)).is_err());
    }
    #[test] fn dossiers_de_travail_valides() {
        let with = |l: &[&str]| Settings { iris_work_dirs: l.iter().map(|x| x.to_string()).collect(), ..Settings::default() }.validate();
        let t = tempfile::tempdir().unwrap();
        let tmp = t.path().to_string_lossy().into_owned();
        assert!(with(&[]).is_ok());
        assert!(with(&[&tmp]).is_ok());
        for ko in [r"C:\Windows", r"c:\program files\x", r"D:\", r"\srv\share", "//srv/share", "relatif", r"C:\a\..\Windows"] {
            assert!(with(&[ko]).is_err(), "{ko}");
        }
        let many: Vec<String> = (0..21).map(|i| format!(r"{tmp}\{i}")).collect();
        assert!(with(&many.iter().map(String::as_str).collect::<Vec<_>>()).is_err());
    }
    #[test] fn raccourci_partiel_garde_le_reste() {
        let s: Settings = serde_json::from_str(r#"{"user_name":"Nathan","hotkey":{"alt":true}}"#).unwrap();
        assert_eq!(s.user_name, "Nathan");
        assert!(s.hotkey.alt);
        assert_eq!(s.hotkey.key, "Space");
    }
    #[test] fn json_corrompu_donne_les_defauts_et_garde_une_copie() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        c.execute("INSERT INTO settings(id, json) VALUES (1, '{pas du json')", []).unwrap();
        assert_eq!(load_settings(&c).unwrap(), Settings::default());
        let (json, broken): (String, Option<String>) =
            c.query_row("SELECT json, broken_json FROM settings WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(broken.as_deref(), Some("{pas du json"));
        assert_eq!(json, serde_json::to_string(&Settings::default()).unwrap());
    }
    #[test] fn absent_donne_les_defauts_et_les_insere() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        assert_eq!(load_settings(&c).unwrap(), Settings::default());
        let n: i64 = c.query_row("SELECT count(*) FROM settings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
    #[test] fn validation() {
        assert!(Settings::default().validate().is_ok());
        let bad = |f: fn(&mut Settings)| { let mut s = Settings::default(); f(&mut s); s.validate().is_err() };
        assert!(bad(|s| s.timeline_start_hour = 23));
        assert!(bad(|s| s.timeline_end_hour = 25));
        assert!(bad(|s| s.hold_threshold_ms = 99));
        assert!(bad(|s| s.hold_threshold_ms = 1001));
        assert!(bad(|s| s.hover_delay_ms = 99));
        assert!(bad(|s| s.hover_delay_ms = 3001));
        assert!(bad(|s| s.mail_poll_minutes = 1));
        assert!(bad(|s| s.mail_poll_minutes = 6));
        assert!(Settings { mail_poll_minutes: 2, ..Settings::default() }.validate().is_ok());
        assert!(Settings { mail_poll_minutes: 5, ..Settings::default() }.validate().is_ok());
        assert_eq!(Settings::default().mail_poll_minutes, 3);
        assert!(bad(|s| s.hotkey.key = "Enter".into()));
        assert!(bad(|s| s.focus_minutes = 4));
        assert!(bad(|s| s.focus_minutes = 121));
        assert!(bad(|s| s.break_minutes = 0));
        assert!(bad(|s| s.break_minutes = 61));
        assert!(bad(|s| s.clipboard_retention_days = 0));
        assert!(bad(|s| s.clipboard_retention_days = 91));
        assert!(bad(|s| s.clipboard_max_items = 19));
        assert!(bad(|s| s.clipboard_max_items = 1001));
        assert!(bad(|s| s.weather_city = "v".repeat(81)));
        assert!(Settings { weather_city: "v".repeat(80), ..Settings::default() }.validate().is_ok());
        let mut s = Settings { weather_city: "  Paris ".into(), ..Settings::default() };
        assert!(s.fix().is_empty());
        assert_eq!(s.weather_city, "Paris");
        fn roots(n: usize) -> Vec<String> { (0..n).map(|i| format!("C:\\p{i}")).collect() }
        assert!(bad(|s| s.search_roots = roots(21)));
        assert!(bad(|s| s.projects_roots = roots(21)));
        assert!(bad(|s| s.search_roots = vec!["relatif\\dossier".into()]));
        assert!(bad(|s| s.projects_roots = vec![format!("C:\\{}", "d".repeat(258))]));
        assert!(bad(|s| s.projects_roots = vec!["C:\\".into()]));
        assert!(bad(|s| s.projects_roots = vec!["C:/".into()]));
        assert!(bad(|s| s.projects_roots = vec!["C:\\Users\\..\\Windows".into()]));
        assert!(bad(|s| s.projects_roots = vec!["C:\\a\\..\\..".into()]));
        assert!(Settings { projects_roots: vec!["C:\\Users\\x".into()], search_roots: vec!["D:\\".into()], ..Settings::default() }.validate().is_ok());
        assert!(Settings { search_roots: roots(20), projects_roots: vec![format!("C:\\{}", "d".repeat(257))], ..Settings::default() }.validate().is_ok());
        let mut s = Settings { search_roots: vec![" C:\\a ".into(), "c:\\A".into(), "  ".into()], ..Settings::default() };
        assert!(s.fix().is_empty());
        assert_eq!(s.search_roots, vec!["C:\\a".to_string()]);
        assert!(Settings { clipboard_retention_days: 1, clipboard_max_items: 20, ..Settings::default() }.validate().is_ok());
        assert!(Settings { clipboard_retention_days: 90, clipboard_max_items: 1000, ..Settings::default() }.validate().is_ok());
        // Sans Alt/Ctrl/Win : refusé, Maj seule ne compte pas.
        assert!(bad(|s| s.hotkey = HotkeySetting { alt: false, shift: true, key: "A".into(), ..HotkeySetting::default() }));
        assert!(bad(|s| s.hotkey.alt = false));
        let ok = |h: HotkeySetting| Settings { hotkey: h, ..Settings::default() }.validate().is_ok();
        assert!(ok(HotkeySetting { alt: false, key: "F5".into(), ..HotkeySetting::default() }));
        assert!(ok(HotkeySetting { alt: false, ctrl: true, key: "A".into(), ..HotkeySetting::default() }));
        assert!(ok(HotkeySetting { alt: false, win: true, ..HotkeySetting::default() }));
        assert!(Settings { timeline_end_hour: 24, hover_delay_ms: 3000, ..Settings::default() }.validate().is_ok());
        assert!(Settings { focus_minutes: 5, break_minutes: 60, ..Settings::default() }.validate().is_ok());
        assert!(Settings { focus_minutes: 120, break_minutes: 1, ..Settings::default() }.validate().is_ok());
        assert!(bad(|s| s.resume_prompt_normal = "  ".into()));
        assert!(bad(|s| s.resume_prompt_orchestrator = "x".repeat(2001)));
        // 2000 caractères, mais le lien encodé deux fois dépasserait 2048 : refusé avec un message clair.
        let long = Settings { resume_prompt_normal: "é".repeat(300), ..Settings::default() }.validate().unwrap_err();
        assert!(long.contains("Consigne trop longue une fois encodée"), "{long}");
        assert!(Settings { resume_prompt_normal: "x".repeat(1900), ..Settings::default() }.validate().is_ok());
        assert!(bad(|s| s.google_client_id = "x".repeat(201)));
        assert!(bad(|s| s.spotify_client_id = "ab cd".into()));
        assert!(bad(|s| s.google_client_id = "ab	cd".into()));
        assert!(Settings { google_client_id: "1-x.apps.googleusercontent.com".into(), spotify_client_id: "x".repeat(200), ..Settings::default() }.validate().is_ok());
        let mut s = Settings { spotify_client_id: " abc ".into(), ..Settings::default() };
        assert!(s.fix().is_empty());
        assert_eq!(s.spotify_client_id, "abc");
        let d = Settings::default();
        assert_eq!((d.google_client_id.as_str(), d.spotify_client_id.as_str()), ("", ""));
    }
    #[test] fn chemin_de_zenytt() {
        let with = |p: &str| Settings { zenytt_path: p.into(), ..Settings::default() }.validate();
        assert!(with("").is_ok());
        assert!(with(r"C:\Users\x\AppData\Local\Zenytt\Zenytt.EXE").is_ok());
        assert!(with("Zenytt.exe").is_err());
        assert!(with(r"C:\Zenytt\Zenytt.bat").is_err());
        assert!(with(r"\\srv\z.exe").is_err());
        assert!(with(r"\\?\C:\Z\Zenytt.exe").is_err());
        assert!(with(&format!(r"C:\{}.exe", "a".repeat(260))).is_err());
        let mut s = Settings { zenytt_path: r"  C:\Z\Zenytt.exe ".into(), ..Settings::default() };
        assert!(s.fix().is_empty());
        assert_eq!(s.zenytt_path, r"C:\Z\Zenytt.exe");
        assert_eq!(Settings::default().zenytt_path, "");
    }
    #[test] fn reglages_de_claude() {
        let with = |p: &str, m: &str| Settings { claude_path: p.into(), iris_claude_model: m.into(), ..Settings::default() }.validate();
        assert!(with("", "").is_ok());
        assert!(with(r"C:\c\claude.exe", "claude-opus-4-5").is_ok());
        assert!(with("", "sonnet[1m]").is_ok());
        assert!(with(r"\\srv\claude.exe", "").is_err());
        assert!(with(r"C:\c\claude.cmd", "").is_err());
        assert!(with("", "a b").is_err());
        assert!(with("", "--dangerously-skip-permissions").is_err());
        assert!(with("", "[1m]").is_err());
        assert!(with("", &"a".repeat(101)).is_err());
        assert_eq!((Settings::default().claude_path.as_str(), Settings::default().iris_claude_model.as_str()), ("", ""));
    }
    #[test] fn import_de_domaines_sans_doublon_ni_depassement() {
        let site = |u: &str| Site { name: "x".into(), url: u.into(), slow_ms: 1500 };
        let mut v = vec![site("https://A.example.com/")];
        let n = merge_domains(&mut v, &["a.example.com".into(), "b.example.com".into(), "b.example.com".into(), "bad host".into(), "u:p@c.example.com".into()]);
        assert_eq!(n, 1);
        assert_eq!(v[1].url, "https://b.example.com");
        assert!(Settings { sites: v.clone(), ..Settings::default() }.validate().is_ok());
        let many: Vec<String> = (0..40).map(|i| format!("s{i}.example.com")).collect();
        assert_eq!(merge_domains(&mut v, &many), 28);
        assert_eq!(v.len(), 30);
    }
    #[test] fn sites_http_seulement_et_30_au_plus() {
        let site = |n: &str, u: &str| Site { name: n.into(), url: u.into(), slow_ms: 1500 };
        let with = |v: Vec<Site>| Settings { sites: v, ..Settings::default() }.validate();
        assert!(with(vec![site("Blog", "https://blog.example.org"), site("Nexus", "http://nexus.example.com:8080/health")]).is_ok());
        for u in ["ftp://x.example.com", "javascript:alert(1)", "file:///C:/x", "example.com", "https://", "https://x.example.com/".repeat(20).as_str()] {
            assert!(with(vec![site("X", u)]).is_err(), "{u}");
        }
        assert!(with(vec![site("  ", "https://a.example.com")]).is_err());
        assert!(with(vec![site(&"n".repeat(41), "https://a.example.com")]).is_err());
        assert!(with(vec![Site { slow_ms: 299, ..site("A", "https://a.example.com") }]).is_err());
        assert!(with(vec![Site { slow_ms: 10_001, ..site("A", "https://a.example.com") }]).is_err());
        assert!(with(vec![site("A", "https://a.example.com"), site("B", "https://a.example.com")]).is_err());
        let many = |n: usize| (0..n).map(|i| site("S", &format!("https://s{i}.example.com"))).collect::<Vec<_>>();
        assert!(with(many(30)).is_ok());
        assert!(with(many(31)).is_err());
        assert!(with(vec![site("A", "https://Blog.example.com"), site("B", "https://blog.example.com/")]).is_err(), "hôte et barre finale");
        assert!(with(vec![site("A", "https://blog.example.com/a"), site("B", "https://blog.example.com/b")]).is_ok());
        assert!(with(vec![site("A", "https://user:pw@blog.example.com")]).is_err(), "identifiants");
        assert!(with(vec![site("A", "https://user@blog.example.com")]).is_err());
        let mut s = Settings { sites: vec![site(" A ", " https://a.example.com ")], ..Settings::default() };
        assert!(s.fix().is_empty());
        assert_eq!((s.sites[0].name.as_str(), s.sites[0].url.as_str()), ("A", "https://a.example.com"));
        let s: Settings = serde_json::from_str(r#"{"sites":[{"name":"A","url":"https://a.example.com"}]}"#).unwrap();
        assert_eq!(s.sites[0].slow_ms, 1500);
    }
    #[test] fn invites_de_reprise_par_defaut() {
        let d = Settings::default();
        assert_eq!(d.resume_prompt_normal, "Reprends là où tu en étais sur ce projet. Appuie-toi sur le plan de développement et l'historique git. Commence par me dire où on en est.");
        assert_eq!(d.resume_prompt_orchestrator, format!("{} Travaille en mode orchestrateur, comme décrit dans mon CLAUDE.md global : graphify pour t'orienter, sous-agents haiku/sonnet/opus selon la tâche, parallélisation des tâches indépendantes, et relecture avant intégration.", d.resume_prompt_normal));
    }
    #[test] fn delai_de_survol_forge_en_base_revient_au_defaut() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        c.execute("INSERT INTO settings(id, json) VALUES (1, ?1)", [r#"{"hover_delay_ms":50}"#]).unwrap();
        assert_eq!(load_settings(&c).unwrap().hover_delay_ms, Settings::default().hover_delay_ms);
    }
    #[test] fn champs_invalides_en_base_remplaces_un_par_un() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        let json = r#"{"user_name":"Nathan","clipboard_retention_days":0,"clipboard_max_items":50,"hold_threshold_ms":400,"hover_delay_ms":5,"focus_minutes":2,"break_minutes":10,"timeline_start_hour":9,"timeline_end_hour":30,"hotkey":{"alt":false,"key":"Space"}}"#;
        c.execute("INSERT INTO settings(id, json) VALUES (1, ?1)", [json]).unwrap();
        let s = load_settings(&c).unwrap();
        let d = Settings::default();
        assert_eq!((s.user_name.as_str(), s.hold_threshold_ms), ("Nathan", 400));
        assert_eq!(s.hover_delay_ms, d.hover_delay_ms);
        assert_eq!((s.focus_minutes, s.break_minutes), (d.focus_minutes, 10));
        assert_eq!((s.timeline_start_hour, s.timeline_end_hour), (d.timeline_start_hour, d.timeline_end_hour));
        assert_eq!(s.hotkey, d.hotkey);
        assert_eq!((s.clipboard_retention_days, s.clipboard_max_items), (d.clipboard_retention_days, 50));
        let broken: Option<String> = c.query_row("SELECT broken_json FROM settings WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(broken, None);
    }
    #[test] fn aller_retour() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        let s = Settings { user_name: "Zoé".into(), hold_threshold_ms: 400, ..Settings::default() };
        save_settings(&c, &s).unwrap();
        assert_eq!(load_settings(&c).unwrap(), s);
    }
}
