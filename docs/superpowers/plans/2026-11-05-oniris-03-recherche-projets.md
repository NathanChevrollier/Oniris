# Oniris — Plan 3 : Recherche, favoris, projets

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Livrer trois choses :
- la barre de commande comme **recherche universelle** : fichiers, dossiers, applications, projets, favoris, notes et commandes « / », en moins de 50 ms ;
- les **favoris**, importés de Nexus_Dashboard et des navigateurs ;
- la carte **Projets** et son bouton **« Reprendre »**, qui ouvre VS Code sur le projet et relance la dernière conversation Claude avec une consigne normale ou orchestrateur.

**Architecture :** On suit le schéma des modules du plan 2 : un cœur `crates/core/src/<module>/`, la colle Tauri dans `src-tauri/src/modules/<module>.rs`, et l'interface dans `src/modules/<module>/`.

- **Index de fichiers.** Il vit **en mémoire** et n'est pas en base. Un thread de fond basse priorité le construit, un observateur (`notify`) le met à jour, et un instantané sur disque permet de redémarrer vite.
- **Correspondance floue.** Elle utilise `nucleo-matcher`. Le classement combine ce score et la fréquence d'usage, stockée en SQLite.
- **Lancement.** Tout passe par `ShellExecuteW` (fichiers, dossiers, applications `.lnk`, URL) ou par un `Command` avec `CREATE_NO_WINDOW` (git, `code`, `wt`). Aucune fenêtre console ne doit apparaître.
- **« Reprendre ».** L'action utilise le gestionnaire d'URI de l'extension Claude pour VS Code, vérifié sur la machine (extension `anthropic.claude-code` 2.1.318) : `vscode://anthropic.claude-code/open?prompt=…&session=…`. Si ce n'est pas possible, on se rabat sur Windows Terminal avec `claude --continue`.

**Tech Stack :** Celle des plans 1 et 2, plus :
- `nucleo-matcher` pour la correspondance floue ;
- `ignore` pour le parcours parallèle avec exclusions ;
- `notify` pour l'observation des fichiers, via ReadDirectoryChangesW ;
- `postcard` et `serde` pour l'instantané de l'index ;
- `lnk` ou un parseur minimal maison pour le nom cible des raccourcis. Le choix est laissé à l'implémenteur, à condition qu'il soit léger.

**Spec :** [`docs/superpowers/specs/2026-11-03-oniris-design.md`](../specs/2026-11-03-oniris-design.md), §6.5, §6.6, §6.7, §7.4, §7.5, §9 et §10.

**Plans précédents et journaux :** [plan 1](2026-11-03-oniris-01-fondations.md), [plan 2](2026-11-04-oniris-02-modules-locaux.md) et [journal 2](2026-11-04-oniris-02-modules-locaux-journal.md). Le journal liste les reports vers ce plan.

## Global Constraints

- Les contraintes globales du plan 2 restent valables : Windows 11, budgets du §9, une seule liste `CORE_MIGRATIONS`, nouvelles commandes déclarées à trois endroits, journal sans contenu utilisateur, R8, et commits signés sans co-signature.
- Commandes Tauri qui touchent la base : toujours `#[tauri::command(async)]`.
- Le chemin d'ouverture du panneau ne verrouille jamais la base.
- **Recherche :** moins de 50 ms entre la frappe et les résultats, mesuré sur un index synthétique de 300 000 entrées en build release.
- **Indexation :**
  - priorité de thread basse (`THREAD_MODE_BACKGROUND_BEGIN`) ;
  - n'ouvre jamais le contenu des fichiers : métadonnées seulement ;
  - ignore les fichiers « en ligne seulement » de OneDrive (`FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`, `FILE_ATTRIBUTE_RECALL_ON_OPEN`, `FILE_ATTRIBUTE_OFFLINE`) pour ne jamais les rapatrier ;
  - ne suit pas les jonctions ni les liens symboliques ;
  - exclusions par défaut : `node_modules`, `target`, `.git`, `dist`, `build`, `.next`, `__pycache__`, `.venv`, `AppData`, `$Recycle.Bin`, plus les dossiers et fichiers cachés ou système.
- **Migrations de ce plan :**
  - 10 : `usage` (fréquence) ;
  - 11 : `favorites` ;
  - 12 : `project_prefs`.
- **Nouveaux réglages :**
  - `search_roots: Vec<String>` (vide = dossier utilisateur, résolu à l'exécution) ;
  - `projects_roots: Vec<String>` (vide = à choisir dans la carte) ;
  - `resume_prompt_normal: String` et `resume_prompt_orchestrator: String`, avec les textes par défaut de la tâche 8.

  Aucun chemin personnel n'est écrit en dur.
- **Lancement :**
  - aucun processus externe n'est lancé sans `CREATE_NO_WINDOW` (ou `DETACHED_PROCESS` pour les applications graphiques) ;
  - les arguments passent par `Command::arg`, jamais par une chaîne de commande concaténée ;
  - les URL ouvertes sont limitées à `http`, `https` et `mailto` ;
  - aucun `file:`, `javascript:` ni schéma inconnu pour les favoris.
- **Outils, tous `Free`** (lire, ouvrir, reprendre, d'après la spec §7.5) : `search.query`, `files.open`, `apps.launch`, `favorites.open`, `favorites.add` (undo), `favorites.delete` (undo), `projects.list`, `projects.status`, `projects.resume`.
- **Confidentialité :**
  - l'index, les sessions Claude et les favoris restent locaux ;
  - les favicons sont récupérées **directement sur le site du favori**, une seule fois, puis mises en cache, sans service tiers. Ce point est documenté dans `docs/legal/confidentialite.md`.
- **R8 pendant l'exécution :**
  - pas de `tauri dev` ;
  - ne lancer ni VS Code, ni Windows Terminal, ni aucune application ou URL ;
  - aucune requête réseau (favicons testées sur des réponses enregistrées) ;
  - ne pas parcourir le vrai dossier utilisateur dans les tests : arborescences temporaires seulement ;
  - lire des fichiers existants pour vérifier un format (extension VS Code, export Nexus, signets d'un navigateur) est permis ; les copier ou les modifier ne l'est pas.

## Review Focus

1. **Un dossier énorme ou piégé dans les racines** : OneDrive avec des fichiers en ligne seulement, un million de fichiers, une boucle de jonctions ou un accès refusé. Résultat attendu : pas de boucle, pas de rapatriement OneDrive, une mémoire bornée (environ 150 octets par entrée au plus), et un parcours qui continue malgré les erreurs. Tests dans la tâche 2.
2. **Frappe pendant que l'index se construit** : des résultats partiels arrivent immédiatement, sans blocage, et le panneau indique « indexation… ». Tests dans les tâches 1, 2 et 4.
3. **Favori importé avec un schéma dangereux** (`javascript:`, `file:`, `vbscript:`, `data:`) : il est refusé à l'import comme à l'ouverture. Tests dans les tâches 5 et 6.
4. **Dépôt git anormal** (aucun commit, HEAD détaché, `.git` corrompu, ou dossier qui n'est pas un dépôt) : pas de plantage, champs vides, et aucune fenêtre console. Tests dans la tâche 7.
5. **Nom de dossier hostile** (espaces, `&`, `"`, `%`, `^`, accents, chemin long) passé à VS Code, au lien `vscode://` ou à Windows Terminal : correctement encodé et échappé, sans injection de commande possible. Tests dans la tâche 8.

---

## Arborescence ajoutée par ce plan

```
crates/core/src/
  search/{mod.rs,index.rs,rank.rs,usage.rs,tools.rs}   index mémoire, score, fréquence (migration 10)
  apps/{mod.rs,tools.rs}                                applications du menu Démarrer
  favorites/{mod.rs,import.rs,tools.rs}                 favoris (migration 11), imports
  projects/{mod.rs,plan.rs,claude.rs,git.rs,resume.rs,tools.rs}  projets (migration 12)
  launch.rs                                             validation d'URL et de chemin (pur)
src-tauri/src/
  launch.rs                                             ShellExecuteW, Command sans console
  modules/{search.rs,indexer.rs,apps.rs,favorites.rs,projects.rs}
src/
  panel/CommandPalette.tsx  (remplace la logique de CommandBar)
  modules/favorites/{api.ts,FavoritesCard.tsx}
  modules/projects/{api.ts,ProjectsCard.tsx,ProjectTile.tsx}
docs/verifications/plan-03-recherche-projets.md
```

---

### Task 1 : Cœur de recherche (index mémoire, score, fréquence)

**Files :** Create `crates/core/src/search/{mod.rs,index.rs,rank.rs,usage.rs}`. Modify `crates/core/src/db.rs` (migration 10), `crates/core/src/lib.rs`, `Cargo.toml` (`nucleo-matcher`). Test : les mêmes fichiers.

**Interfaces :**
- `pub enum ItemKind { File, Dir, App, Project, Favorite, Note, Command }`, avec `Serialize` et le nom en minuscules.
- `pub struct Item { pub kind: ItemKind, pub key: String /* chemin, url, id… unique par kind */, pub title: String, pub subtitle: String }`.
- `pub struct FileIndex`, avec :
  - un stockage compact : `Vec<Entry>`, où `Entry { path: Box<str>, name_start: u16, is_dir: bool }` et le nom est la tranche finale de `path` ;
  - `insert(path, is_dir)`, `remove_prefix(path)`, `len()` ;
  - `snapshot()/restore()` via `postcard`.
- `pub fn query(index: &FileIndex, extra: &[Item], usage: &UsageMap, q: &str, limit: usize) -> Vec<Hit>`, où `Hit { item: Item, score: u32 }`.
  - Le score combine `nucleo` sur le nom (bonus si la correspondance est un préfixe ou une frontière de mot) et le chemin complet avec un poids moindre.
  - S'y ajoute `frecency(count, last_ms, now)` : `count * decay`, avec une demi-vie de 7 jours. Le poids est fixé par une constante documentée.
  - Requête vide : renvoie les éléments les plus récents par usage.
- Migration 10 : `usage(kind TEXT NOT NULL, key TEXT NOT NULL, count INTEGER NOT NULL, last_ms INTEGER NOT NULL, PRIMARY KEY(kind, key)) STRICT`.
  - `usage::record(c, kind, key, now)`.
  - `usage::load(c) -> UsageMap`, de type `HashMap<(ItemKind, String), (u32, i64)>`, chargée une fois puis tenue à jour en mémoire.
  - `usage::prune(c, keep: 5000)`.

- [ ] **Step 1 : Tests.**
  - « oniris » classe `Projets/Oniris` (dossier) avant `…/oniris-old/readme.md`.
  - « plan fond » trouve `2026-11-03-oniris-01-fondations.md` (mots dans le désordre).
  - Un élément utilisé souvent remonte devant un score flou légèrement meilleur, et la fréquence décroît avec le temps.
  - `remove_prefix` retire tout un sous-arbre.
  - L'aller-retour `snapshot` puis `restore` est fidèle.
  - Test de performance `#[ignore]` : 300 000 chemins synthétiques et une requête de 3 lettres en moins de 50 ms en `--release`. Documenter la commande `cargo test -p oniris-core --release search_perf -- --ignored`.
  - Test de mémoire : environ 150 octets par entrée au plus (`size_of` plus la longueur des chaînes).
- [ ] **Step 2 :** RED, puis GREEN. Lancer le test de performance en release et consigner le résultat.
- [ ] **Step 3 :** Commit : `feat(recherche): ajoute l'index mémoire et le classement flou`

---

### Task 2 : Indexeur de fichiers (parcours, observation, instantané)

**Files :** Create `src-tauri/src/modules/indexer.rs`. Modify `crates/core/src/settings.rs` (+ TS) pour `search_roots`, `state.rs`, `lib.rs`, `Cargo.toml` (`ignore`, `notify`, `postcard`). Test : `indexer.rs`, sur des arborescences `tempfile`.

**Interfaces :**
- `pub struct Indexer` : un `Arc<RwLock<FileIndex>>` et un état `Indexing { done: u64 } | Ready { count: u64 }`, émis par `oniris://index` **vers le panneau seulement**, et au plus deux fois par seconde.
- `pub fn start(app, roots: Vec<PathBuf>, snapshot_path: PathBuf)` déroule ces étapes :
  1. restaurer l'instantané s'il existe, ce qui rend l'index utilisable tout de suite ;
  2. lancer un thread basse priorité pour un parcours complet, avec `ignore::WalkBuilder` : `hidden(true)`, `git_ignore(true)`, `follow_links(false)`, exclusions globales et filtre d'attributs ;
  3. remplacer l'index à la fin du parcours ;
  4. écrire l'instantané (`%APPDATA%\Oniris\index.bin`) ;
  5. démarrer `notify::RecommendedWatcher` en mode récursif sur chaque racine. Les événements sont regroupés pendant 500 ms, puis appliqués en `insert` ou `remove_prefix`.
- Fonction pure `pub fn should_skip(name: &str, attrs: u32, is_dir: bool) -> bool` : exclusions par nom et attributs `HIDDEN | SYSTEM | RECALL_ON_DATA_ACCESS | RECALL_ON_OPEN | OFFLINE | REPARSE_POINT`.
- Les erreurs d'accès sont comptées et journalisées une fois par racine. Le parcours continue.
- Racines effectives : `search_roots`, ou le dossier utilisateur (`dirs::home_dir`) si la liste est vide. On y ajoute toujours les `projects_roots`.
- Commande `index_status()`, asynchrone.

- [ ] **Step 1 : Tests sur arborescence temporaire.**
  - `node_modules`, `.git` et `target` exclus.
  - Fichier marqué caché exclu (via `SetFileAttributesW` sur le fichier temporaire).
  - Une jonction (`mklink /J` au sens de `std::os::windows::fs::symlink_dir` si les droits le permettent, sinon test `#[ignore]` documenté) n'est pas suivie.
  - `should_skip` testée pour chaque attribut de la liste.
  - Création puis suppression d'un fichier observées en moins d'1 s (test `#[tokio::test]` sur un dossier temporaire uniquement).
  - Instantané rechargé au démarrage.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(recherche): indexe les fichiers en tâche de fond`

---

### Task 3 : Applications et lancement sûr

**Files :** Create `crates/core/src/launch.rs`, `crates/core/src/apps/{mod.rs,tools.rs}`, `src-tauri/src/launch.rs`, `src-tauri/src/modules/apps.rs`. Modify `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`. Test : les fichiers du cœur.

**Interfaces :**
- `launch.rs` (cœur, pur) :
  - `pub fn safe_url(s: &str) -> Option<String>` : accepte seulement `http`, `https` et `mailto` après analyse (crate `url`, déjà tirée par `reqwest`).
  - `pub fn safe_path(p: &Path, allowed_roots: &[PathBuf]) -> bool` : chemin absolu existant, sans composante `..` après canonicalisation.
- `launch.rs` (Tauri) :
  - `pub fn open_path(p)` et `pub fn open_url(u)` : `ShellExecuteW` avec le verbe `open`, sur un thread annexe (jamais le fil principal).
  - `pub fn spawn_hidden(program, args, cwd) -> io::Result<Child>` : `CREATE_NO_WINDOW`.
  - `pub fn spawn_detached(program, args, cwd)` : `DETACHED_PROCESS`.
- Applications :
  - `apps::scan(dirs) -> Vec<Item>` lit les raccourcis `.lnk` de `%ProgramData%\Microsoft\Windows\Start Menu\Programs` et de `%AppData%\Microsoft\Windows\Start Menu\Programs`, récursivement.
  - Le titre est le nom du fichier sans extension. Les doublons sont retirés, ainsi que « Uninstall », « Désinstaller », « Help » et « Read me ».
  - L'inventaire est rafraîchi au démarrage, puis toutes les 6 h ou quand le panneau s'ouvre si l'inventaire a plus de 6 h. Les applications Store (UWP) sont exclues : `shortcut: pas d'apps UWP, à ajouter via shell:AppsFolder si l'utilisateur le demande`.
- Outils :
  - `files.open {path}`, Free : `safe_path` contre les racines de recherche et de projets.
  - `apps.launch {key}`, Free : la clé doit être une entrée connue de l'inventaire.
- Le `.lnk` ouvert via ShellExecute lance l'application cible.

- [ ] **Step 1 : Tests.**
  - `safe_url` : `javascript:alert(1)`, `file:///c:/x`, `vbscript:`, `data:text/html,…`, ` JAVASCRIPT:` (espaces et casse) → `None`. `https://github.com` → `Some`.
  - `safe_path` : chemin hors racine, chemin avec `..`, chemin inexistant → faux.
  - `apps::scan` sur un dossier temporaire contenant de faux `.lnk` (seul le nom compte) : déduplication et exclusions.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(lancement): ouvre fichiers, applications et liens sans risque`

---

### Task 4 : Palette de commande (recherche universelle dans la barre)

**Files :** Create `src/panel/CommandPalette.tsx`, `src/panel/palette.css`, `src-tauri/src/modules/search.rs`. Modify `src/panel/CommandBar.tsx`, `src/panel/Panel.tsx`, `lib.rs`, `build.rs`, `capabilities/panel.json`. Test : `src/panel/CommandPalette.test.tsx`, et les tests Rust de la commande.

**Interfaces :**
- Commande `search_query(q: String, scope: Option<String>, req: u32) -> { req, hits: Vec<Hit>, indexing: bool }`, asynchrone. Elle combine :
  - l'index de fichiers ;
  - les applications ;
  - les projets (tâche 7) ;
  - les favoris (tâche 5) ;
  - les notes, via `notes::search` limité à 5 résultats ;
  - les commandes « / ».
- Préfixes de portée : `p ` projets, `f ` fichiers, `a ` applications, `b ` favoris, `n ` notes, `/` commandes.
- Commande `search_open(kind, key)`, asynchrone : elle enregistre l'usage, puis ouvre l'élément via l'outil correspondant, en passant par `dispatch_as_user` pour que le journal reste complet. Ensuite elle ferme le panneau, sauf pour les commandes « / » qui ouvrent une carte.
- Commandes « / » : une liste statique en TypeScript qui renvoie vers des outils existants :
  - `/minuteur <durée>` → `timer.start` ;
  - `/focus` → `focus.start` ;
  - `/note <texte>` → `notes.create` ;
  - `/rappel <texte>` → `reminders.create` ;
  - `/événement <texte>` → `agenda.create_event` ;
  - `/météo <ville>` → enregistre `weather_city` ;
  - `/presse-papiers` → focus sur la carte.
- Interface :
  - résultats dans une liste au-dessus de la barre (au plus 8 visibles), avec icône par type, titre surligné sur les caractères trouvés, et sous-titre (chemin raccourci) ;
  - ↑ et ↓ pour naviguer, Entrée pour ouvrir, Ctrl+Entrée pour ouvrir le dossier parent d'un fichier, Échap pour vider puis fermer ;
  - Ctrl+P dans le panneau place le focus dans la barre avec la portée `p ` ;
  - une requête par frappe, sans délai, avec numéro de requête : les réponses périmées sont ignorées ;
  - texte sans résultat validé par Entrée : message « IRIS arrive bientôt — essaie /note, /rappel… ».
- La pilule et le chemin d'ouverture ne déclenchent aucune recherche.

- [ ] **Step 1 : Tests TypeScript.**
  - La navigation au clavier est correcte.
  - Les réponses périmées sont ignorées.
  - Les préfixes de portée sont transmis.
  - `/minuteur 10 min` appelle `timers.start("10 min")`.
  - Ctrl+P donne le focus avec `p `.
  - Échap vide le champ puis ferme le panneau.
- [ ] **Step 2 : Tests Rust.** Combinaison des sources, et `indexing` à vrai tant que le premier parcours n'est pas fini.
- [ ] **Step 3 :** RED, puis GREEN. Aperçu Edge sans interface avec de fausses données (`!isTauri`), capture `p3t4-palette.png`. Commit : `feat(recherche): transforme la barre en recherche universelle`

---

### Task 5 : Favoris

**Files :** Create `crates/core/src/favorites/{mod.rs,tools.rs}`, `src-tauri/src/modules/favorites.rs`, `src/modules/favorites/{api.ts,FavoritesCard.tsx,favorites.css}`. Modify `db.rs` (migration 11), `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`, `Panel.tsx`, `CardGrid.tsx`. Test : fichiers du cœur, `favorites.test.tsx`.

**Interfaces :**
- Migration 11 : `favorites(id INTEGER PRIMARY KEY, title TEXT NOT NULL, url TEXT NOT NULL, category TEXT, position INTEGER NOT NULL, icon TEXT, deleted INTEGER NOT NULL DEFAULT 0, created_ms INTEGER NOT NULL) STRICT`, avec un index unique `(url) WHERE deleted = 0`.
  - `icon` contient une URL `data:image/png;base64,…` ou `NULL`.
- Fonctions du cœur : `add`, `set_deleted`, `reorder(ids_in_order)`, `rename`, `list()` (non supprimés, triés par catégorie puis position), `get`. Pour ajouter, l'URL passe par `safe_url`.
- Outils :
  - `favorites.open {id}`, Free : `open_url` ;
  - `favorites.add {title?, url}`, Free, avec undo ; sans titre, on prend le domaine ;
  - `favorites.delete {id}`, Free, avec undo.
- Interface : `FavoritesCard` prend l'emplacement `vps` jusqu'au plan 4. Ce plan 4 y ajoutera une section VPS au-dessus, et l'emplacement s'appellera alors « Favoris ». Elle contient :
  - une grille d'icônes façon dock (squircle 40 px avec favicon, ou initiales sur une teinte dérivée du domaine) ;
  - des titres de catégorie quand il y en a plusieurs ;
  - un réordonnancement par glisser-déposer à la souris (événements pointeur, pas de bibliothèque), qui appelle `favorites_reorder` ;
  - au clavier, Alt+← et Alt+→ déplacent un favori ;
  - un bouton « + » qui ouvre une petite ligne URL et titre, plus un clic droit (ou menu « … ») pour renommer ou supprimer ;
  - un clic qui ouvre le favori, puis ferme le panneau.

- [ ] **Step 1 :** Tests.
  - URL dangereuse refusée.
  - Doublon refusé.
  - Réordonnancement persisté.
  - Suppression puis undo.
  - Côté TypeScript : un clic appelle `open`, le glisser-déposer appelle `reorder` avec le nouvel ordre, et le déplacement au clavier fonctionne.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(favoris): ajoute le dock de favoris`

---

### Task 6 : Imports de favoris et favicons

**Files :** Create `crates/core/src/favorites/import.rs`, `crates/core/tests/fixtures/favorites/*`. Modify `favorites/mod.rs`, `src-tauri/src/modules/favorites.rs`, `FavoritesCard.tsx`, `docs/legal/confidentialite.md`. Test : `import.rs` avec les fixtures.

**Interfaces :**
- `pub fn from_nexus(json) -> Result<Vec<NewFavorite>>` :
  - lit le format d'export de Nexus_Dashboard. **Lire d'abord** `Projets/Nexus_Dashboard/src/server/modules/dashboards/transfer.ts` et les schémas de ses widgets « Lien » et « Signets » pour connaître la forme exacte ;
  - écrire une fixture qui respecte ce schéma ;
  - les noms de catégorie viennent des catégories Nexus.
- `pub fn from_chromium(json) -> Result<Vec<NewFavorite>>` : fichier `Bookmarks` de Chrome, Edge et Brave (barre de favoris et « autres »). Le chemin du dossier devient la catégorie.
- `pub fn from_firefox(places_sqlite_copy: &Path)` : lit `moz_bookmarks` et `moz_places` sur une **copie** temporaire (Firefox verrouille le fichier). Ouverture en lecture seule.
- `pub fn import(c, items) -> ImportReport { added, skipped_dup, skipped_unsafe }`.
- Tauri :
  - `favorites_import_browsers()` détecte les profils (`%LOCALAPPDATA%\Google\Chrome\User Data\*\Bookmarks`, Edge, Brave, `%APPDATA%\Mozilla\Firefox\Profiles\*\places.sqlite`) et les importe ;
  - `favorites_import_file(path)` sert au JSON Nexus, choisi par une boîte de dialogue. `tauri-plugin-dialog` est déjà utilisé dans MacroWorld ; l'ajouter.
- Favicons :
  - une seule tentative par favori, sur `https://<domaine>/favicon.ico`, puis sur la balise `<link rel=icon>` de la page d'accueil (analyse minimale) ;
  - délai maximal 5 s, 256 Ko au plus, redimensionnement en PNG 64 px, stockage dans `icon` ;
  - en cas d'échec, on garde les initiales et on ne réessaie pas avant 7 jours (`kv`) ;
  - lancées en tâche de fond après un import ou un ajout. **Aucun service tiers.**
- `confidentialite.md` : nouvelle section « Favoris ». Elle indique la lecture locale des signets des navigateurs et la requête de favicon envoyée directement à chaque site, une fois.

- [ ] **Step 1 :** Tests sur fixtures : Nexus, Chromium (dossiers imbriqués) et Firefox (une petite base `places.sqlite` créée par le test). `javascript:` ignoré et compté. Doublons ignorés. L'extraction du `<link rel=icon>` est testée sur du HTML enregistré.
- [ ] **Step 2 :** RED, puis GREEN. Aucune requête réseau réelle. Commit : `feat(favoris): importe Nexus et les navigateurs, récupère les icônes`

---

### Task 7 : Projets — inventaire, plan, git, sessions Claude

**Files :** Create `crates/core/src/projects/{mod.rs,plan.rs,claude.rs,git.rs}`, `src-tauri/src/modules/projects.rs`. Modify `db.rs` (migration 12), `settings.rs` (+ TS) pour `projects_roots`, `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`. Test : fichiers du cœur sur arborescences temporaires.

**Interfaces :**
- Migration 12 : `project_prefs(path TEXT PRIMARY KEY, pinned INTEGER NOT NULL DEFAULT 0, hidden INTEGER NOT NULL DEFAULT 0) STRICT`.
- `pub struct Project { path, name, last_activity_ms, branch: Option<String>, dirty: u32, activity_14d: [u8; 14], plan: Option<PlanProgress>, last_session: Option<SessionInfo>, pinned, hidden }`.
- Détection : un sous-dossier direct d'une racine est un projet s'il contient au moins l'un de ces éléments :
  - `.git` ;
  - `package.json` ;
  - `Cargo.toml` ;
  - `*.sln` ;
  - `project.godot` ;
  - `pyproject.toml` ;
  - `go.mod`.

  `AppData` et les dossiers cachés sont exclus.
- `plan.rs` :
  - `pub struct PlanProgress { file, done, total, current: Option<String> }` ;
  - le fichier retenu est le plus récent de `docs/superpowers/plans/*.md`, hors `*-journal.md` et `*feuille-de-route*` ;
  - les étapes sont les lignes `- [ ]` et `- [x]` ;
  - `current` est le titre `### Task N : …` de la première tâche qui a une case non cochée.
- `claude.rs` :
  - `pub fn sessions_dir(project_path) -> PathBuf` encode le chemin comme Claude Code : les caractères `:`, `\`, `/` et `.` deviennent `-`. Le résultat est comparé **sans tenir compte de la casse** aux dossiers de `~/.claude/projects/`, car la lettre du lecteur varie (`c--` ou `C--`) ;
  - `pub struct SessionInfo { id: String /* nom du .jsonl */, updated_ms, title: Option<String> }` : la session la plus récente ;
  - `title` est la dernière entrée `{"type":"summary","summary":…}`, sinon le premier message utilisateur tronqué à 80 caractères ;
  - le fichier est lu **par la fin** : au plus 256 Ko, jamais en entier.
- `git.rs` :
  - `git -C <path> status --porcelain=v2 --branch` donne la branche et le nombre de fichiers modifiés ;
  - `git -C <path> log --since=14.days --format=%ct` donne l'activité sur 14 jours ;
  - les deux commandes passent par `spawn_hidden`, avec un délai maximal de 3 s ;
  - fonctions pures d'analyse : `parse_status(&str)` et `bucket_activity(&[i64], now) -> [u8; 14]`.
- Cache : les informations git et Claude sont recalculées en tâche de fond quand le panneau s'ouvre, si elles ont plus de 2 min, puis signalées par un nouveau sujet `Topic::Projects` (à ajouter à `Topic` et au type `Topic` en TypeScript). Le chemin d'ouverture ne bloque jamais.
- Commandes : `projects_list()`, `projects_pin(path, bool)`, `projects_hide(path, bool)`, `projects_set_roots(roots)`.
- Outils :
  - `projects.list`, Free ;
  - `projects.status {name}`, Free : texte « où j'en suis » qui assemble le plan, la dernière session, git et la dernière activité. IRIS le lira au plan 6.

- [ ] **Step 1 : Tests.**
  - Détection sur une arborescence temporaire.
  - `parse_status` sur des sorties réelles : dépôt propre, fichiers modifiés, HEAD détaché, aucun commit.
  - `bucket_activity`.
  - Analyse d'un plan sur un fichier modèle : 7 cases sur 12, tâche courante.
  - `sessions_dir` pour `C:\Users\x\Projets\Oniris` et `c:\…` (casse du lecteur).
  - Résumé de session tiré d'un `.jsonl` de test.
  - Fichier de session de 10 Mo : seule la fin est lue (vérifié par le temps d'exécution ou par un lecteur instrumenté).
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(projets): inventorie les projets, leur plan, git et la session Claude`

---

### Task 8 : « Reprendre » (VS Code + Claude)

**Files :** Create `crates/core/src/projects/resume.rs`, `crates/core/src/projects/tools.rs`. Modify `src-tauri/src/modules/projects.rs`, `settings.rs` (+ TS). Test : `resume.rs`.

**Interfaces :**
- Réglages par défaut :
  - `resume_prompt_normal` = « Reprends là où tu en étais sur ce projet. Appuie-toi sur le plan de développement et l'historique git. Commence par me dire où on en est. »
  - `resume_prompt_orchestrator` = le texte normal suivi de « Travaille en mode orchestrateur, comme décrit dans mon CLAUDE.md global : graphify pour t'orienter, sous-agents haiku/sonnet/opus selon la tâche, parallélisation des tâches indépendantes, et relecture avant intégration. »
- `pub enum Mode { Normal, Orchestrator }`.
- `pub fn vscode_uri(prompt: &str, session: Option<&str>) -> String` construit `vscode://anthropic.claude-code/open?prompt=<encodé>&session=<id>`.
  - L'encodage se fait avec `url::form_urlencoded` ; aucune concaténation brute.
  - `session` est un UUID, validé par une expression simple avant d'être inclus.
- `pub fn plan(project, mode, prompts, vscode_ok: bool) -> Vec<LaunchStep>` produit :
  - quand `vscode_ok` est vrai : `[Code { args: [path] }, Uri { uri: vscode_uri(..) }]`, avec un délai de 1,5 s entre les deux pour laisser VS Code ouvrir la fenêtre ;
  - sinon, le repli : `[Code { args: [path] }, Terminal { program: "wt", args: ["-d", path, "claude", "--continue", prompt] }]`.
- Détection de `vscode_ok` : l'extension `anthropic.claude-code-*` existe dans `%USERPROFILE%\.vscode\extensions` **et** son `extension.js` contient `case"/open"`. Le résultat est mis en cache pendant 1 h.
- Exécution :
  - `code` est lancé via `spawn_hidden("cmd", ["/C", "code", path])`. **Ou mieux**, résoudre `code.cmd` vers `Code.exe` (`%LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe`) et le lancer avec `spawn_detached`. Documenter le choix ; aucun chemin utilisateur n'est écrit en dur, on passe par les variables d'environnement.
  - L'URI est ouverte via `open_url`, en élargissant `safe_url` au seul préfixe exact `vscode://anthropic.claude-code/open`.
  - `wt` est lancé via `spawn_detached`, avec les arguments séparés.
- Outil `projects.resume {name, mode}`, Free.

- [ ] **Step 1 : Tests.**
  - Encodage de l'URI avec des guillemets, `&`, `%`, des accents et un retour à la ligne.
  - Un identifiant de session invalide est exclu.
  - Le plan produit en mode normal et en mode orchestrateur, avec et sans `vscode_ok`.
  - Un nom de dossier hostile (`a&b "c" %PATH% ^x`) reste un seul argument, sans métacaractère interprété. Si `cmd /C` est utilisé, tester l'échappement ; sinon montrer que `Code.exe` reçoit le chemin en un seul argument.
  - Détection de `vscode_ok` sur un faux dossier d'extension.
- [ ] **Step 2 :** RED, puis GREEN. **Ne lancer ni VS Code, ni le terminal.** Commit : `feat(projets): reprend un projet dans VS Code avec Claude`

---

### Task 9 : Carte Projets

**Files :** Create `src/modules/projects/{api.ts,ProjectsCard.tsx,ProjectTile.tsx,projects.css}`. Modify `Panel.tsx` (emplacement `projects`). Test : `projects.test.tsx`.

**Interface :** elle reprend la maquette.
- Jusqu'à 3 tuiles : épinglées d'abord, puis triées par activité. Un lien « Tous les projets (N) » ouvre la palette avec la portée `p `.
- Chaque tuile affiche :
  - une icône aux initiales sur un dégradé dérivé du nom ;
  - le nom et l'activité relative ;
  - un anneau de progression du plan (`done/total`, ou `—` sans plan) ;
  - la ligne « Tu en étais à : … » (tâche courante du plan, sinon titre de la dernière session) ;
  - une carte d'activité de 14 cases ;
  - des étiquettes pour la branche et le nombre de modifications ;
  - un bouton partagé **Reprendre** | **⧉ Orchestr.**
- Menu « … » de la tuile : épingler, masquer, ouvrir le dossier.
- Si `projects_roots` est vide : un état vide invite à choisir le dossier de tes projets, avec un champ en ligne et un bouton « Parcourir » (boîte de dialogue de dossier).

- [ ] **Step 1 :** Tests.
  - Rendu de 3 tuiles.
  - Les boutons appellent `projects.resume` avec le bon mode.
  - État vide quand il n'y a pas de racine.
  - Le texte « Tu en étais à » vient du plan s'il existe, sinon de la session.
- [ ] **Step 2 :** RED, puis GREEN. Aperçu Edge sans interface, capture `p3t9-panel.png` comparée à la maquette. Commit : `feat(projets): ajoute la carte Projets et le bouton Reprendre`

---

### Task 10 : Intégration, confidentialité, liste de vérification

**Files :** Modify `docs/legal/confidentialite.md`, `docs/superpowers/plans/2026-11-03-oniris-feuille-de-route.md`. Create `docs/verifications/plan-03-recherche-projets.md`.

- [ ] **Step 1 :** Capture complète du panneau en 2560×1440 et en 1920×1080, avec toutes les cartes. Le panneau doit toujours tenir en 1440p.
- [ ] **Step 2 :** Compléter `confidentialite.md` :
  - index de fichiers local, sans ouvrir le contenu des fichiers ;
  - lecture locale des sessions Claude ;
  - lecture locale des signets des navigateurs ;
  - requêtes de favicons vers les sites.
- [ ] **Step 3 :** Rédiger la liste de vérification au format des plans 1 et 2, avec ces points :
  - recherche d'un fichier, d'une application et d'un projet ;
  - Ctrl+P ;
  - commandes « / » ;
  - recherche pendant l'indexation ;
  - OneDrive non rapatrié ;
  - pas de console qui clignote ;
  - import des signets Chrome, Edge et Firefox, plus Nexus ;
  - favicons ;
  - glisser-déposer ;
  - « Reprendre » qui ouvre VS Code sur le bon projet, avec Claude sur la dernière session et la consigne ;
  - mode orchestrateur ;
  - dossier au nom bizarre ;
  - mesure de la recherche en moins de 50 ms en build `perf`.

  Marquer « ⚠️ à vérifier en priorité » : « Reprendre », OneDrive, la console invisible et la mesure de 50 ms.
- [ ] **Step 4 :** Cocher le plan 3 dans la feuille de route. Commit : `docs(verifications): liste les contrôles manuels du plan 3`

---

## Fin du plan 3 : critères

- Ces commandes passent toutes : `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm test` et `pnpm build`.
- Le test de performance de la recherche, en release, tient sous 50 ms et son résultat est consigné.
- Les migrations 10 à 12 sont contiguës et testées.
- Aucun lancement de processus ne se fait sans `CREATE_NO_WINDOW` ou `DETACHED_PROCESS`, ni ne passe par une chaîne concaténée.
- `confidentialite.md` est à jour.
- Le graphe graphify est mis à jour avec `graphify update .`.
