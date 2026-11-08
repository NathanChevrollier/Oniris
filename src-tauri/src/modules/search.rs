//! Recherche universelle de la barre de commande : index de fichiers + sources (applications, notes, commandes « / »).
//! Journal : jamais la requête ni une clé (chemins = contenu utilisateur).
use crate::commands::dispatch_as_user;
use crate::modules::indexer::Indexer;
use crate::state::AppState;
use oniris_core::apps::Inventory;
use oniris_core::launch::is_executable;
use oniris_core::search::{self, FileIndex, Hit, Item, ItemKind, UsageMap};
use oniris_core::time_util::now_ms;
use oniris_core::tools::{ToolError, ToolOutcome};
use oniris_core::{notes, Db};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

const LIMIT: usize = 30;
/// Notes gardées dans le cache de la recherche.
// shortcut: plafond de 100 000 notes (texte entier en mémoire), upgrade : index inversé si une base dépasse ça.
const NOTES_CACHED: usize = 100_000;
const CLIP: usize = 80;
/// Miroir des commandes « / » de `src/panel/CommandPalette.tsx` (clé, description, argument attendu ou "") :
/// elles s'exécutent côté interface. Un test vérifie les deux listes dans les deux sens.
const COMMANDS: [(&str, &str, &str); 7] = [
    ("minuteur", "Lancer un minuteur", "durée"),
    ("focus", "Démarrer une session focus", ""),
    ("note", "Noter quelque chose", "texte"),
    ("rappel", "Créer un rappel", "texte"),
    ("événement", "Ajouter un événement à l'agenda", "texte"),
    ("météo", "Changer la ville de la météo", "ville"),
    ("presse-papiers", "Ouvrir l'historique du presse-papiers", ""),
];

#[derive(Debug, Serialize)]
pub struct SearchResult {
    req: u32,
    hits: Vec<Hit>,
    indexing: bool,
}

/// Suite donnée par l'interface à `search_open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Opened {
    /// Ouvert : le panneau se ferme.
    Done,
    /// Carte de confirmation affichée (exécutable) : le panneau reste ouvert.
    Pending,
    /// L'interface montre la carte Notes.
    Note,
    /// L'interface exécute la commande « / ».
    Command,
}

/// La portée « fichiers » couvre aussi les dossiers.
fn wants(scope: Option<ItemKind>, k: ItemKind) -> bool {
    match scope {
        None => true,
        Some(ItemKind::File) => matches!(k, ItemKind::File | ItemKind::Dir),
        Some(s) => s == k,
    }
}

/// Reconstruit le cache des notes (appelé sur `Topic::Notes`, jamais en tenant le verrou de la base).
pub fn refresh_notes(app: &AppHandle) {
    if let Some(st) = app.try_state::<AppState>() {
        // Verrou du cache pris avant la lecture : un instantané plus ancien ne peut pas écraser un plus récent.
        let mut cache = st.notes.write();
        *cache = Arc::new(note_items(&st.db));
    }
}

/// Première ligne en titre, le reste (une ligne) en sous-titre, tous deux entiers : le titre est classé en flou,
/// le texte entier par mots présents (voir `oniris_core::search`).
/// L'affichage est raccourci à `CLIP` caractères dans `run`, pas ici.
pub fn note_items(db: &Db) -> Vec<Item> {
    let found = notes::recent(&db.lock(), NOTES_CACHED);
    let found = found.inspect_err(|e| tracing::warn!("recherche : notes indisponibles : {e}")).unwrap_or_default();
    found
        .into_iter()
        .map(|n| {
            let (first, rest) = n.body.trim().split_once('\n').unwrap_or((n.body.trim(), ""));
            let rest = rest.split_whitespace().collect::<Vec<_>>().join(" ");
            let subtitle = if rest.is_empty() { "Note".into() } else { rest };
            Item { kind: ItemKind::Note, key: n.id.to_string(), title: first.trim().to_string(), subtitle }
        })
        .collect()
}

/// Même sous-titre que l'interface : « description · argument ».
fn command_items() -> Vec<Item> {
    COMMANDS
        .iter()
        .map(|(key, hint, arg)| Item {
            kind: ItemKind::Command,
            key: key.to_string(),
            title: format!("/{key}"),
            subtitle: if arg.is_empty() { hint.to_string() } else { format!("{hint} · {arg}") },
        })
        .collect()
}

/// Sources hors index, une ligne par source.
/// Les applications ne sont pas copiées : seul l'`Arc` de l'inventaire est cloné.
fn extras(apps: &Inventory, favorites: &Arc<Vec<Item>>, projects: &Arc<Vec<Item>>, notes: &Arc<Vec<Item>>, scope: Option<ItemKind>) -> Vec<Arc<Vec<Item>>> {
    let mut v = Vec::new();
    if wants(scope, ItemKind::App) {
        v.push(apps.items.read().clone());
    }
    if wants(scope, ItemKind::Favorite) {
        v.push(favorites.clone());
    }
    if wants(scope, ItemKind::Project) {
        v.push(projects.clone());
    }
    if wants(scope, ItemKind::Note) {
        v.push(notes.clone());
    }
    if wants(scope, ItemKind::Command) {
        v.push(Arc::new(command_items()));
    }
    v
}

fn run(index: &FileIndex, extra: &[Arc<Vec<Item>>], usage: &UsageMap, q: &str, scope: Option<ItemKind>, now: i64) -> Vec<Hit> {
    let empty = FileIndex::new();
    let index = if wants(scope, ItemKind::File) { index } else { &empty };
    // Récents d'une portée : on trie tout l'usage puis on filtre, sinon les autres types prendraient les places.
    let limit = if scope.is_some() && q.trim().is_empty() { usage.len().max(LIMIT) } else { LIMIT };
    let sources: Vec<&[Item]> = extra.iter().map(|s| s.as_slice()).collect();
    let mut hits = search::query_at(index, &sources, usage, q, limit, now);
    hits.retain(|h| wants(scope, h.item.kind));
    hits.truncate(LIMIT);
    for h in hits.iter_mut().filter(|h| h.item.kind == ItemKind::Note) {
        for f in [&mut h.item.title, &mut h.item.subtitle] {
            if let Some((i, _)) = f.char_indices().nth(CLIP) { f.truncate(i); }
        }
    }
    hits
}

/// Vrai tant que le premier parcours complet n'est pas fini (l'instantané restauré peut être ancien).
fn indexing(ix: Option<&Indexer>) -> bool {
    ix.is_none_or(|i| !i.first_walk_done())
}

enum Route {
    Tool(&'static str, Value),
    Ui(Opened),
}

fn route(kind: ItemKind, key: &str) -> Result<Route, ToolError> {
    Ok(match kind {
        // files.open refuse les exécutables : files.run demande confirmation.
        ItemKind::File if is_executable(Path::new(key)) => Route::Tool("files.run", json!({ "path": key })),
        ItemKind::File | ItemKind::Dir => Route::Tool("files.open", json!({ "path": key })),
        ItemKind::App => Route::Tool("apps.launch", json!({ "key": key })),
        ItemKind::Note => Route::Ui(Opened::Note),
        ItemKind::Command => Route::Ui(Opened::Command),
        ItemKind::Favorite => Route::Tool("favorites.open", json!({ "id": key.parse::<i64>().map_err(|_| ToolError::BadArgs("favori inconnu".into()))? })),
        ItemKind::Project => Route::Tool("files.open", json!({ "path": key })),
    })
}

/// Une requête par frappe, numérotée par l'interface : une requête dépassée avant son tour ne calcule rien.
#[tauri::command]
pub async fn search_query(app: AppHandle, q: String, scope: Option<String>, req: u32) -> Result<SearchResult, String> {
    let scope = scope.map(|s| ItemKind::parse(&s).ok_or("portée inconnue")).transpose()?;
    let latest = &app.state::<AppState>().search_req;
    // Le compteur de l'interface repart de 1 si la page est rechargée.
    if req == 1 {
        latest.store(1, Ordering::SeqCst);
    } else {
        latest.fetch_max(req, Ordering::SeqCst);
    }
    // Le parcours de l'index utilise des fils (scoped threads) : hors du runtime asynchrone.
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let indexing = indexing(st.indexer.get());
        if st.search_req.load(Ordering::SeqCst) > req {
            return SearchResult { req, hits: Vec::new(), indexing };
        }
        let favs = st.favorites.read().clone();
        let projects = st.projects.read().items.clone();
        let notes = st.notes.read().clone();
        let extra = extras(&st.apps, &favs, &projects, &notes, scope);
        let empty = FileIndex::new();
        let index = st.indexer.get().map(|i| i.index.read());
        let hits = run(index.as_deref().unwrap_or(&empty), &extra, &st.usage.read(), &q, scope, now_ms());
        SearchResult { req, hits, indexing }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Ouvre un résultat via son outil (journalisé comme une action de l'utilisateur), puis retient l'usage.
/// Ferme le panneau une fois ouvert ; pas pendant une confirmation, ni pour une note ou une commande « / ».
#[tauri::command]
pub async fn search_open(app: AppHandle, kind: String, key: String) -> Result<Opened, String> {
    let kind = ItemKind::parse(&kind).ok_or("type inconnu")?;
    let opened = match route(kind, &key).map_err(|e| e.to_string())? {
        Route::Ui(o) => o,
        Route::Tool(tool, args) => match dispatch_as_user(&app, tool, args).await {
            ToolOutcome::Done { .. } => Opened::Done,
            ToolOutcome::NeedsConfirmation(_) => Opened::Pending,
            ToolOutcome::Rejected { .. } => return Err("action refusée".into()),
            ToolOutcome::Failed { error, .. } => return Err(error),
        },
    };
    // shortcut: un exécutable confirmé (Pending puis approuvé) n'entre pas dans les récents ; à brancher sur la
    // résolution de la confirmation si l'utilisateur le demande.
    if opened == Opened::Pending {
        return Ok(opened);
    }
    // Usage retenu seulement après une ouverture réussie : pas de récent vers un fichier disparu.
    let st = app.state::<AppState>();
    let now = now_ms();
    if let Err(e) = search::record(&st.db.lock(), kind, &key, now) {
        tracing::warn!("recherche : usage non enregistré : {e}");
    }
    search::bump(&mut st.usage.write(), kind, &key, now);
    if opened == Opened::Done {
        crate::windows::close_panel(&app).map_err(|e| e.to_string())?;
    }
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn item(kind: ItemKind, key: &str, title: &str) -> Item {
        Item { kind, key: key.into(), title: title.into(), subtitle: String::new() }
    }
    fn db() -> Db {
        let mut c = rusqlite::Connection::open_in_memory().unwrap();
        oniris_core::migrate(&mut c, oniris_core::CORE_MIGRATIONS).unwrap();
        Arc::new(parking_lot::Mutex::new(c))
    }
    fn kinds(h: &[Hit]) -> Vec<ItemKind> {
        let mut k: Vec<_> = h.iter().map(|h| h.item.kind).collect();
        k.sort();
        k.dedup();
        k
    }

    #[test]
    fn combine_fichiers_applications_notes_et_commandes() {
        let db = db();
        notes::insert(&db.lock(), "Rapport trimestriel\nchiffres à revoir").unwrap();
        notes::insert(&db.lock(), "Penser au pain").unwrap();
        let apps = Inventory::default();
        apps.set(vec![item(ItemKind::App, r"C:\m\Rapports Pro.lnk", "Rapports Pro")], 1);
        let mut ix = FileIndex::new();
        ix.insert(r"C:\u\docs\rapport.pdf", false);
        ix.insert(r"C:\u\rapports", true);
        ix.insert(r"C:\u\vacances.jpg", false);
        let usage = UsageMap::new();

        let extra = extras(&apps, &Arc::default(), &Arc::default(), &Arc::new(note_items(&db)), None);
        let h = run(&ix, &extra, &usage, "rapport", None, 1);
        assert_eq!(kinds(&h), [ItemKind::File, ItemKind::Dir, ItemKind::App, ItemKind::Note]);
        let note = h.iter().find(|h| h.item.kind == ItemKind::Note).unwrap();
        assert_eq!(note.item.title, "Rapport trimestriel");
        assert!(note.item.key.parse::<i64>().is_ok(), "clé de note = id");
        assert!(!h.iter().any(|h| h.item.title.contains("pain") || h.item.key.contains("vacances")));

        // Commandes « / » : présentes dans la recherche universelle.
        let h = run(&ix, &extras(&apps, &Arc::default(), &Arc::default(), &Arc::new(note_items(&db)), None), &usage, "minut", None, 1);
        assert_eq!(h[0].item.kind, ItemKind::Command);
        assert_eq!(h[0].item.key, "minuteur");
    }

    #[test]
    fn les_notes_sont_trouvees_sur_tout_le_texte_et_toutes_anciennes_comprises() {
        let db = db();
        notes::insert(&db.lock(), "Ancienne note zebulon").unwrap();
        for i in 0..600 { notes::insert(&db.lock(), &format!("Note courante {i}")).unwrap(); }
        let long = format!("Titre
{} motfinalrare", "mot ".repeat(50));
        notes::insert(&db.lock(), &long).unwrap();
        let (apps, ix, usage) = (Inventory::default(), FileIndex::new(), UsageMap::new());
        let cache = Arc::new(note_items(&db));
        let go = |q: &str| run(&ix, &extras(&apps, &Arc::default(), &Arc::default(), &cache, Some(ItemKind::Note)), &usage, q, Some(ItemKind::Note), 1);
        let h = go("motfinalrare");
        assert_eq!(h.len(), 1, "mot en fin de texte (position > 200)");
        assert_eq!(h[0].item.title, "Titre");
        assert!(h[0].item.subtitle.chars().count() <= CLIP, "affichage raccourci");
        assert_eq!(go("zebulon").len(), 1, "note plus ancienne que les 500 dernières");
    }

    #[test]
    fn la_portee_filtre_les_sources() {
        let db = db();
        notes::insert(&db.lock(), "rapport de note").unwrap();
        let apps = Inventory::default();
        apps.set(vec![item(ItemKind::App, r"C:\m\Rapport.lnk", "Rapport")], 1);
        let mut ix = FileIndex::new();
        ix.insert(r"C:\u\rapport.pdf", false);
        ix.insert(r"C:\u\rapports", true);
        let usage = UsageMap::new();
        let go = |s: ItemKind| run(&ix, &extras(&apps, &Arc::default(), &Arc::default(), &Arc::new(note_items(&db)), Some(s)), &usage, "rapport", Some(s), 1);
        assert_eq!(kinds(&go(ItemKind::File)), [ItemKind::File, ItemKind::Dir], "f : fichiers et dossiers");
        assert_eq!(kinds(&go(ItemKind::App)), [ItemKind::App]);
        assert_eq!(kinds(&go(ItemKind::Note)), [ItemKind::Note]);
        assert!(go(ItemKind::Project).is_empty(), "aucun projet dans la source");
    }

    #[test]
    fn les_favoris_sont_cherchables_et_filtres_par_portee() {
        let db = db();
        let favs = Arc::new(vec![Item { kind: ItemKind::Favorite, key: "4".into(), title: "Nexus".into(), subtitle: "nexus.example.com".into() }]);
        let (apps, ix, usage) = (Inventory::default(), FileIndex::new(), UsageMap::new());
        let h = run(&ix, &extras(&apps, &favs, &Arc::default(), &Arc::new(note_items(&db)), None), &usage, "nexus", None, 1);
        assert_eq!(kinds(&h), [ItemKind::Favorite]);
        let h = run(&ix, &extras(&apps, &favs, &Arc::default(), &Arc::new(note_items(&db)), Some(ItemKind::App)), &usage, "nexus", Some(ItemKind::App), 1);
        assert!(h.is_empty());
    }

    #[test]
    fn requete_vide_recents_de_la_portee() {
        let db = db();
        let apps = Inventory::default();
        apps.set(vec![item(ItemKind::App, "a.lnk", "Firefox")], 1);
        let mut usage = UsageMap::new();
        search::bump(&mut usage, ItemKind::File, r"C:\u\a.txt", 1);
        search::bump(&mut usage, ItemKind::App, "a.lnk", 1);
        search::bump(&mut usage, ItemKind::App, "disparue.lnk", 1);
        let ix = FileIndex::new();
        let h = run(&ix, &extras(&apps, &Arc::default(), &Arc::default(), &Arc::new(note_items(&db)), None), &usage, "", None, 2);
        assert_eq!(h.len(), 2, "app inconnue de l'inventaire ignorée");
        let h = run(&ix, &extras(&apps, &Arc::default(), &Arc::default(), &Arc::new(note_items(&db)), Some(ItemKind::App)), &usage, "", Some(ItemKind::App), 2);
        assert_eq!(h.iter().map(|h| h.item.title.as_str()).collect::<Vec<_>>(), ["Firefox"]);
    }

    #[test]
    fn indexation_vraie_jusqu_a_la_fin_du_premier_parcours() {
        assert!(indexing(None), "indexeur pas encore lancé");
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("a.txt"), "x").unwrap();
        let snap = tempfile::tempdir().unwrap();
        let ix = Indexer::spawn(vec![t.path().to_path_buf()], snap.path().join("index.bin"), Arc::new(|_| {}));
        let start = Instant::now();
        while indexing(Some(&ix)) {
            assert!(start.elapsed() < Duration::from_secs(10), "premier parcours jamais fini");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(matches!(ix.status(), crate::modules::indexer::IndexStatus::Ready { .. }));
    }

    #[test]
    fn routage_par_type() {
        let tool = |k, key: &str| match route(k, key) {
            Ok(Route::Tool(t, a)) => (t, a),
            _ => panic!("outil attendu"),
        };
        assert_eq!(tool(ItemKind::File, r"C:\a.pdf"), ("files.open", json!({"path": r"C:\a.pdf"})));
        assert_eq!(tool(ItemKind::File, r"C:\setup.EXE").0, "files.run", "exécutable : confirmation");
        assert_eq!(tool(ItemKind::Dir, r"C:\d").0, "files.open");
        assert_eq!(tool(ItemKind::App, "a.lnk"), ("apps.launch", json!({"key": "a.lnk"})));
        assert!(matches!(route(ItemKind::Note, "3"), Ok(Route::Ui(Opened::Note))));
        assert!(matches!(route(ItemKind::Command, "focus"), Ok(Route::Ui(Opened::Command))));
        assert_eq!(tool(ItemKind::Project, r"C:\p\Zenytt"), ("files.open", json!({"path": r"C:\p\Zenytt"})));
        assert_eq!(tool(ItemKind::Favorite, "7"), ("favorites.open", json!({"id": 7})));
        assert!(route(ItemKind::Favorite, "abc").is_err());
    }

    #[test]
    fn commandes_synchronisees_avec_l_interface() {
        let ts = include_str!("../../../src/panel/CommandPalette.tsx");
        // Rust -> TS : clé, description et argument identiques.
        for (key, hint, arg) in COMMANDS {
            let want = if arg.is_empty() { format!("key: \"{key}\", hint: \"{hint}\", run:") } else { format!("key: \"{key}\", hint: \"{hint}\", arg: \"{arg}\"") };
            assert!(ts.contains(&want), "commande différente ou absente de CommandPalette.tsx : {key}");
        }
        // TS -> Rust : aucune clé côté interface seulement.
        let ts_keys: Vec<&str> = ts.split("key: \"").skip(1).map(|s| &s[..s.find('"').unwrap()]).collect();
        for k in &ts_keys {
            assert!(COMMANDS.iter().any(|(key, _, _)| key == k), "commande absente de COMMANDS (Rust) : {k}");
        }
        assert_eq!(ts_keys.len(), COMMANDS.len());
    }
}
