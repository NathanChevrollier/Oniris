# Oniris — Plan 2 : Modules locaux

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Remplir le panneau avec les modules qui ne dépendent d'aucun compte en ligne : agenda et frise, rappels avec notifications, notes, minuteur et focus, historique du presse-papiers, état du PC, météo. Chaque module est exposé à IRIS par des outils enregistrés auprès du courtier.

**Architecture :** Chaque module suit le même schéma en trois couches.
1. **Cœur** (`crates/core/src/<module>/`, sans Tauri) : modèle, accès SQLite par fonctions qui prennent une `&Connection`, logique pure, et outils `Tool`.
2. **Colle Tauri** (`src-tauri/src/modules/<module>.rs`) : commandes et tâches de fond.
3. **Interface** (`src/modules/<module>/`) : `api.ts` et `Card.tsx`.

Toute modification de données, qu'elle vienne de l'interface ou d'un outil, signale un **sujet** (`topic`) via le trait `Events`. L'interface ne recharge un sujet que **panneau ouvert** : fermé, elle le marque périmé et le recharge à l'ouverture. Les échéances (rappels, minuteurs) passent par un **ordonnanceur unique** qui ne se réveille qu'à la prochaine échéance. Rien ne tourne à intervalle fixe quand le panneau est fermé, à part l'écoute du presse-papiers, qui est événementielle.

**Tech Stack :** Celle du plan 1, plus :
- `chrono` (dates locales) et `tracing` / `tracing-subscriber` / `tracing-appender` (journal fichier) ;
- `reqwest` (features `rustls-tls`, `json`, sans `default-features`) pour Open-Meteo ;
- `sysinfo` (CPU, RAM, mémoire des processus) et `image` (feature `png` seulement) pour les miniatures du presse-papiers ;
- de nouvelles features de la crate `windows` : `Win32_System_DataExchange`, `Win32_System_Memory`, `Win32_System_Performance`, `Win32_Graphics_Gdi` (déjà présente) et `Win32_System_Ole`.

**Spec :** [`docs/superpowers/specs/2026-11-03-oniris-design.md`](../specs/2026-11-03-oniris-design.md) (§6.2, §6.9 à §6.13, §7.4, §7.5, §9, §10). Plan précédent et journal : [`2026-11-03-oniris-01-fondations.md`](2026-11-03-oniris-01-fondations.md), [`2026-11-03-oniris-01-fondations-journal.md`](2026-11-03-oniris-01-fondations-journal.md). Maquette : [`docs/design/maquette-panneau.html`](../../design/maquette-panneau.html).

## Global Constraints

- Windows 11 uniquement.
- Données dans `%APPDATA%\Oniris` ; journal fichier dans `%APPDATA%\Oniris\logs`.
- **Budgets (spec §9) :**
  - CPU au repos panneau fermé ≈ 0 % (< 0,5 %) ;
  - ouverture du panneau < 100 ms, **impérative** : aucune requête réseau ni lecture lourde sur le chemin d'ouverture, les données arrivent après l'affichage ;
  - RAM repère < 200 Mo.
- **SQLite :** une seule liste de migrations globale `CORE_MIGRATIONS`, versions contiguës. Ce plan ajoute les versions **3 à 8, dans cet ordre** :
  - 3 : `events` ;
  - 4 : `reminders` ;
  - 5 : `notes` et `notes_fts` ;
  - 6 : `timers` ;
  - 7 : `clipboard` ;
  - 8 : `kv`.

  Tables `STRICT`. La base n'est ouverte que par `AppState`.
- **Outils :** noms au format `module.action`. Permissions selon la spec §7.5 : supprimer un événement d'agenda passe par `Confirm`, tout le reste de ce plan est `Free`. Toute action destructrice fournit un `undo`. Les outils sont construits dans `AppState::open` et passés à `ToolBroker::new`.
- **Nouvelle commande Tauri :** l'ajouter à `generate_handler!` dans `src-tauri/src/lib.rs`, à `AppManifest::commands` dans `src-tauri/build.rs` **et** à la capacité de la seule fenêtre qui l'appelle (`src-tauri/capabilities/panel.json` ou `pill.json`).
- **Données hors du PC :** seule la météo (Open-Meteo : nom de ville, coordonnées). Elle doit être décrite dans `docs/legal/confidentialite.md`.
- **Presse-papiers :** ne jamais enregistrer un contenu marqué par les formats Windows `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` (DWORD 0) ou `CanUploadToCloudClipboard` (DWORD 0).
- **Interface :**
  - en français, accent `#ff7448`, style de la maquette ;
  - animations limitées à `transform` et `opacity` ;
  - rien ne se rafraîchit panneau fermé, **sauf** le compte à rebours de la pilule quand un minuteur tourne (une mise à jour par seconde, seulement dans ce cas).
- **Règle R8 du plan 1, toujours en vigueur pendant l'exécution :**
  - pas de `pnpm tauri dev` ;
  - pas de frappe simulée, pas de fenêtre ouverte sur le bureau ;
  - pas de notification réelle déclenchée ;
  - pas de requête réseau réelle pendant les tests : le client Open-Meteo est testé sur des réponses enregistrées ;
  - les vérifications réelles vont dans la liste de la tâche 12 ;
  - les aperçus visuels se font avec Vite et Edge sans interface, comme au plan 1 (tâche 8).
- Commits Conventional Commits en français, sans co-signature, sans push.

## Review Focus

1. **Rappel arrivé à échéance pendant que l'app était fermée ou le PC en veille** → au démarrage ou au réveil, une seule notification marquée « en retard », jamais deux. Tests dans la tâche 5.
2. **« demain à 10 h » la veille d'un changement d'heure (fin octobre ou fin mars)** → l'échéance tombe à 10 h heure locale du lendemain. Test dans la tâche 3.
3. **Copie depuis un gestionnaire de mots de passe, texte énorme (10 Mo), ou presse-papiers verrouillé par une autre app** → rien n'est enregistré dans le premier cas. Dans le deuxième, l'entrée est tronquée à 64 Ko et marquée comme tronquée. Dans le troisième, deux nouvelles tentatives à 50 ms d'intervalle, puis abandon silencieux et journalisé, sans plantage. Tests dans la tâche 8.
4. **Pas de réseau, ville inconnue ou réponse Open-Meteo malformée** → la carte affiche la dernière valeur connue avec son heure, ou « Météo indisponible ». Pas de nouvel essai avant 10 minutes, pas de plantage. Tests dans la tâche 10.
5. **Minuteur en cours quand l'app est quittée** → au redémarrage, il reprend avec le temps restant ; s'il est déjà écoulé, une notification « terminé pendant ton absence » part, une seule fois. Tests dans la tâche 7.

---

## Arborescence ajoutée par ce plan

```
crates/core/src/
  events.rs                 trait Events + Topic
  when.rs                   analyse des dates en français
  kv.rs                     petit stockage clé/valeur (migration 8)
  agenda/{mod.rs,tools.rs}  événements (migration 3)
  reminders/{mod.rs,tools.rs} rappels (migration 4)
  notes/{mod.rs,tools.rs}   notes + FTS5 (migration 5)
  timers/{mod.rs,tools.rs}  minuteurs et focus (migration 6)
  clipboard/{mod.rs,filter.rs,tools.rs} historique (migration 7)
  weather/{mod.rs,codes.rs,tools.rs}   Open-Meteo, parsing pur
  system/{mod.rs,tools.rs}  type SystemStatus + outil
src-tauri/src/
  logging.rs  scheduler.rs  events.rs (impl Events → oniris://changed)
  modules/{mod.rs,agenda.rs,reminders.rs,notes.rs,timers.rs,clipboard.rs,system.rs,weather.rs}
src/
  lib/data.ts               useTopic
  modules/agenda/{api.ts,AgendaCard.tsx,MonthGrid.tsx,DayList.tsx}
  modules/reminders/{api.ts,ReminderList.tsx}
  modules/notes/{api.ts,NotesCard.tsx}
  modules/timers/{api.ts,TimerCard.tsx,PillTimer.tsx}
  modules/clipboard/{api.ts,ClipboardCard.tsx}
  modules/system/{api.ts,PcCard.tsx,Gauge.tsx}
  modules/weather/{api.ts,WeatherBadge.tsx}
docs/legal/confidentialite.md
docs/verifications/plan-02-modules-locaux.md
```

---

### Task 1 : Journal fichier

**Files :** Create `src-tauri/src/logging.rs`. Modify `src-tauri/src/lib.rs` and every `eprintln!` in `src-tauri/src/**` and `crates/core/src/**`. Test : `src-tauri/src/logging.rs`.

**Interfaces :**
- Produces : `pub fn init(dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard>`, appelée en tout premier dans `setup`, avec un garde conservé dans `AppState` (`pub _log_guard: Option<WorkerGuard>`).
  - Rotation quotidienne, fichiers `oniris.log.AAAA-MM-JJ`, 7 fichiers au maximum (`Builder::max_log_files(7)`).
  - Niveau `info`, `debug` en build debug.
  - En build debug, la sortie va aussi dans stderr.
- Produces : `pub fn log_dir() -> PathBuf`, qui renvoie `%APPDATA%\Oniris\logs`.
- Toute la crate utilise `tracing::{info, warn, error, debug}` ; il ne reste plus aucun `eprintln!`. `crates/core` dépend de `tracing` seulement, sans subscriber.
- Un échec d'ouverture de la base au démarrage est journalisé par `error!` avant que l'app ne s'arrête.

- [ ] **Step 1 :** Test `init_cree_le_dossier_et_ecrit` : appeler `init` sur un `tempdir`, journaliser `info!("bonjour")`, abandonner le garde (drop), puis vérifier qu'un fichier du dossier contient « bonjour ». **RED**, puis implémentation, puis **GREEN**.
- [ ] **Step 2 :** Remplacer tous les `eprintln!`. Vérifier avec `rg "eprintln!" src-tauri crates` : aucun résultat attendu.
- [ ] **Step 3 :** Vérifier : `cargo test --workspace` passe, `cargo clippy --workspace --all-targets -- -D warnings` est propre.
- [ ] **Step 4 :** Commit : `feat(journal): écrit le journal dans des fichiers tournants`

---

### Task 2 : Plomberie commune (sujets, ordonnanceur, emplacements de cartes)

**Files :** Create `crates/core/src/events.rs`, `src-tauri/src/events.rs`, `src-tauri/src/scheduler.rs`, `src-tauri/src/modules/mod.rs`, `src/lib/data.ts`. Modify `crates/core/src/lib.rs`, `src-tauri/src/state.rs`, `src/panel/CardGrid.tsx`, `src/panel/Panel.tsx`. Test : `crates/core/src/events.rs`, `src-tauri/src/scheduler.rs`, `src/lib/data.test.ts`, `src/panel/CardGrid.test.tsx`.

**Interfaces :**
- Core :

  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
  #[serde(rename_all = "lowercase")]
  pub enum Topic { Agenda, Reminders, Notes, Timers, Clipboard, Weather }

  pub trait Events: Send + Sync { fn changed(&self, topic: Topic); }

  pub struct NoEvents; // impl Events vide, pour les tests
  ```

- Tauri :
  - `pub struct TauriEvents(AppHandle)` implémente `Events` et émet `oniris://changed` vers `panel` **et** `pill` avec la charge utile `{ "topic": "agenda" }`.
  - `AppState` gagne `pub events: Arc<dyn Events>`.
  - Comme `AppState::open` reçoit déjà `&AppHandle`, `TauriEvents` y est construit avant les outils.
- Ordonnanceur (`src-tauri/src/scheduler.rs`) :
  - `pub struct Scheduler`, avec `Scheduler::start(app: AppHandle) -> Arc<Scheduler>` (tâche tokio unique, lancée avec `tauri::async_runtime::spawn`) ;
  - `pub fn set(&self, key: &'static str, at_ms: Option<u64>)`, qui remplace l'échéance de cette clé et réveille la tâche via `tokio::sync::Notify` ;
  - `pub fn on_due(&self, key: &'static str, f: Arc<dyn Fn(&AppHandle) + Send + Sync>)`. Chaque module rappelle `set` après avoir traité son échéance.
  - Logique pure testable : `pub fn next_due(entries: &HashMap<&'static str, u64>) -> Option<(&'static str, u64)>`.
  - La tâche dort jusqu'à `min(at_ms)`, ou indéfiniment s'il n'y a aucune échéance. Aucun réveil périodique.
  - `AppState` gagne `pub scheduler: OnceLock<Arc<Scheduler>>`, rempli dans `setup` après `manage`.
- Front (`src/lib/data.ts`) :

  ```ts
  export type Topic = "agenda" | "reminders" | "notes" | "timers" | "clipboard" | "weather";
  export function useTopic<T>(topic: Topic, fetcher: () => Promise<T>, initial: T): { data: T; reload: () => void };
  ```

  - charge à la première ouverture du panneau ;
  - sur `oniris://changed` du même sujet : recharge si le panneau est ouvert, sinon marque le sujet périmé et le recharge à la prochaine ouverture ;
  - aucun chargement tant que le panneau n'a jamais été ouvert.
- `CardGrid` :
  - `export type SlotId = "music" | "mails" | "agenda" | "projects" | "pc" | "vps" | "notes" | "timer" | "clipboard";`
  - `<CardGrid slots={Partial<Record<SlotId, ReactNode>>} />` affiche, dans cet ordre et sur trois rangées de 12 colonnes : `music` s4, `mails` s5, `agenda` s3 / `projects` s6, `pc` s3, `vps` s3 / `notes` s4, `clipboard` s5, `timer` s3. Un emplacement absent affiche la carte « Bientôt » de son titre.
  - On exporte `SoonCard({ id })`.

- [ ] **Step 1 :** Écrire les tests qui échouent.
  - Rust : `next_due` sur une table vide renvoie `None`, et sur `{a:500, b:200}` renvoie `("b", 200)`. Un test d'intégration de l'ordonnanceur avec horloge réelle : `set("x", now+50)`, puis `on_due` est appelé une fois dans les 500 ms (`#[tokio::test]`). Ce test n'utilise pas d'`AppHandle` : la boucle prend un `Fn()` générique en interne, et l'API publique enveloppe `AppHandle`.
  - TS : `useTopic` ne charge pas tant que le panneau est fermé, charge à l'ouverture, recharge sur un événement du même sujet panneau ouvert, ignore les autres sujets, et recharge à la réouverture quand le sujet est périmé. On simule `listen` et `usePanelStore`.
  - `CardGrid` : l'emplacement `agenda` remplacé affiche son contenu, les autres affichent « Bientôt », et l'ordre des titres est conforme à la liste ci-dessus.
- [ ] **Step 2 :** **RED**, puis implémentation, puis **GREEN** (`cargo test --workspace`, `pnpm test`).
- [ ] **Step 3 :** Commit : `feat(socle): ajoute les sujets de données, l'ordonnanceur et les emplacements de cartes`

---

### Task 3 : Dates en français

**Files :** Create `crates/core/src/when.rs`. Test : le même fichier.

**Interfaces :**
- Produces :
  - `pub struct When { pub start: DateTime<Local>, pub end: Option<DateTime<Local>>, pub all_day: bool, pub rest: String }`. `rest` est le texte privé de l'expression temporelle, nettoyé (exemple : « appeler la banque »).
  - `pub fn parse_when(text: &str, now: DateTime<Local>) -> Option<When>`.
- Formes à reconnaître, insensibles à la casse et aux accents :
  - « dans N minutes / min / heures / h / jours » ;
  - « à 10h », « à 10 h 30 », « à 10:30 », « 18h » ;
  - « aujourd'hui », « ce soir » (19 h), « demain », « après-demain » ;
  - jours de la semaine (« lundi » désigne le prochain lundi, jamais aujourd'hui), « lundi prochain » ;
  - « le 12 », « le 12 novembre », « le 12/11 » ;
  - les plages « de 14h à 15h30 » et « 14h-15h30 ».
- Règles par défaut :
  - un jour sans heure donne `all_day = true` ;
  - une heure seule déjà passée aujourd'hui désigne le lendemain ;
  - pour un rappel, l'appelant transforme « demain » sans heure en 9 h. C'est la constante `DEFAULT_REMINDER_HOUR = 9`.

- [ ] **Step 1 :** Écrire les tests (`now` = mardi 2026-11-03 14:20, heure locale fixée par une fonction de test qui construit des `DateTime<FixedOffset>` convertis ; documenter l'approche choisie pour rendre les tests indépendants du fuseau de la machine, p. ex. une variante interne `parse_when_tz<Tz: TimeZone>` testée avec `chrono_tz::Europe::Paris` en dev-dependency) :
  - `"dans 40 minutes appeler maman"` → start 15:00, rest `"appeler maman"` ;
  - `"demain à 10h appeler la banque"` → 2026-11-04 10:00, rest `"appeler la banque"` ;
  - `"rappel à 9h"` → 2026-11-04 09:00 (9 h passée aujourd'hui) ;
  - `"lundi réunion"` → 2026-11-09, all_day, rest `"réunion"` ;
  - `"le 12 novembre de 14h à 15h30 dentiste"` → 12/11 14:00–15:30 ;
  - `"vendredi 18h-19h sport"` → 2026-11-06 18:00–19:00 ;
  - **changement d'heure** : now = samedi 2026-03-28 20:00 Europe/Paris, `"demain à 10h"` → 2026-03-29 10:00 heure locale (UTC+2) ;
  - `"rien de temporel ici"` → `None`.
- [ ] **Step 2 :** **RED**, puis implémentation, puis **GREEN**. Un analyseur à base de jetons, sans regex géantes : chaque forme est une petite fonction.
- [ ] **Step 3 :** Commit : `feat(dates): comprend les dates et heures écrites en français`

---

### Task 4 : Agenda local et frise

**Files :** Create `crates/core/src/agenda/{mod.rs,tools.rs}`, `src-tauri/src/modules/agenda.rs`, `src/modules/agenda/{api.ts,AgendaCard.tsx,MonthGrid.tsx,DayList.tsx}`. Modify `crates/core/src/db.rs` (migration 3), `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`, `src/panel/Panel.tsx`. Test : `crates/core/src/agenda/*.rs`, `src/modules/agenda/*.test.tsx`.

**Interfaces :**
- Migration 3 : `events(id INTEGER PRIMARY KEY, title TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER, all_day INTEGER NOT NULL DEFAULT 0, color TEXT, notes TEXT, deleted INTEGER NOT NULL DEFAULT 0, created_ms INTEGER NOT NULL) STRICT`, avec l'index `ON events(start_ms) WHERE deleted = 0`.
- Core :
  - `pub struct Event { id, title, start_ms: i64, end_ms: Option<i64>, all_day: bool, color: Option<String>, notes: Option<String> }` (Serialize/Deserialize) ;
  - `insert(c, &NewEvent) -> i64`, `update_times(c, id, start, end)`, `set_deleted(c, id, bool)`, `get(c, id)`, `between(c, from_ms, to_ms) -> Vec<Event>` (les événements qui chevauchent l'intervalle), `days_with_events(c, month_start_ms, month_end_ms) -> Vec<u32>` (jours du mois, en heure locale).
- Outils (`agenda/tools.rs`), qui reçoivent `Arc<Mutex<Connection>>` et `Arc<dyn Events>` :
  - `agenda.create_event`, Free, args `{ text }` **ou** `{ title, start_ms, end_ms?, all_day? }`. Avec `text`, `parse_when` est appliqué ; s'il n'y a pas de date reconnue → `BadArgs("date introuvable")`. Undo : suppression ;
  - `agenda.move_event`, Free, args `{ id, start_ms, end_ms? }`. Undo : les anciennes valeurs ;
  - `agenda.delete_event`, **Confirm**, args `{ id }`. Suppression logique ; undo = restauration ;
  - `agenda.list`, Free, args `{ from_ms, to_ms }`.

  Chaque outil qui modifie les données appelle `events.changed(Topic::Agenda)`.
- Commandes Tauri :
  - `agenda_between(from_ms, to_ms)`, `agenda_month_days(year, month)` ;
  - `agenda_create(text)` et `agenda_delete(id)`, qui passent **par le courtier** (`dispatch_tool_call` avec `Origin::User`, `Provenance::User`). Une suppression demandée par l'interface déclenche donc la carte de confirmation, conformément à la spec.
- Front :
  - `AgendaCard` (emplacement `agenda`) reprend la maquette : en-tête « Novembre » avec flèches ‹ ›, `MonthGrid` (semaine du lundi au dimanche, jour courant en pastille accent, point sous les jours qui ont un événement), puis `DayList` du jour sélectionné ;
  - un champ « Ajouter… » envoie le texte saisi à `agenda_create` ;
  - `Panel` alimente la `Timeline` avec les événements du jour (`kind: "event"`).

- [ ] **Step 1 :** Tests Rust :
  - migration 3 appliquée et idempotente (le test global « versions contiguës » couvre la numérotation) ;
  - `between` renvoie un événement qui chevauche le début de l'intervalle ;
  - `days_with_events` ;
  - les outils : create avec texte → ligne créée et sujet signalé (un `Events` de test compte les appels) ; texte sans date → `BadArgs` ; delete passe par Confirm et l'undo le restaure (via le courtier, avec `resolve`) ; move puis undo.
- [ ] **Step 2 :** Tests TS : `MonthGrid` place le 1er novembre 2026 (dimanche) en 7e colonne, marque le jour courant et les points ; `DayList` vide affiche « Rien de prévu ».
- [ ] **Step 3 :** **RED**, puis implémentation, puis **GREEN**. Aperçu headless (`pnpm dev` + Edge, données factices hors Tauri) comparé à la carte « Novembre » de la maquette. Capture dans le scratchpad.
- [ ] **Step 4 :** Commit : `feat(agenda): ajoute l'agenda local, le calendrier du mois et la frise`

---

### Task 5 : Rappels et notifications

**Files :** Create `crates/core/src/reminders/{mod.rs,tools.rs}`, `src-tauri/src/modules/reminders.rs`, `src/modules/reminders/{api.ts,ReminderList.tsx}`. Modify `db.rs` (migration 4), `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`, `AgendaCard.tsx`, `Panel.tsx`. Test : fichiers core, `ReminderList.test.tsx`.

**Interfaces :**
- Migration 4 : `reminders(id INTEGER PRIMARY KEY, text TEXT NOT NULL, due_ms INTEGER, done INTEGER NOT NULL DEFAULT 0, notified INTEGER NOT NULL DEFAULT 0, deleted INTEGER NOT NULL DEFAULT 0, created_ms INTEGER NOT NULL) STRICT`, avec l'index `ON reminders(due_ms) WHERE done = 0 AND deleted = 0 AND notified = 0`.
- Core :
  - `pub struct Reminder { id, text, due_ms: Option<i64>, done: bool }` ;
  - `insert`, `set_done`, `set_deleted`, `list_open(c) -> Vec<Reminder>` (non faits, triés par échéance, ceux sans échéance en dernier) ;
  - `next_due(c) -> Option<i64>` ;
  - `take_due(c, now_ms) -> Vec<Reminder>` : marque `notified = 1` **dans la même transaction** et renvoie les rappels concernés. C'est ce qui garantit une seule notification.
  - `pub fn is_late(due_ms: i64, now_ms: i64) -> bool` : vrai si le rappel a plus de 60 s de retard.
- Outils :
  - `reminders.create`, Free, `{ text }`. Pour l'échéance, `parse_when` ; un jour sans heure donne 9 h ; sans date, le rappel n'a pas d'échéance. Undo : suppression ;
  - `reminders.complete`, Free, `{ id, done }`. Undo : l'inverse ;
  - `reminders.delete`, Free, `{ id }`. Suppression logique ; undo = restauration.

  Chaque outil signale `Topic::Reminders` et reprogramme l'échéance.
- Tauri :
  - au démarrage, `take_due(now)` déclenche une notification par rappel en retard (titre « Rappel en retard », corps : le texte), puis `scheduler.set("reminders", next_due)` ;
  - `on_due("reminders")` envoie `notify("Rappel", texte)` pour chaque rappel, puis reprogramme ;
  - sur `WM_POWERBROADCAST` / `PBT_APMRESUMEAUTOMATIC`, on reprogramme aussi. Plus simple : l'ordonnanceur utilise des échéances en temps réel, et chaque réveil recalcule à partir de l'horloge murale. Documenter le choix ;
  - commandes `reminders_list`, `reminders_create(text)`, `reminders_complete(id, done)` et `reminders_delete(id)`, toutes via le courtier.
- Front : `ReminderList` est intégrée sous le calendrier dans `AgendaCard`, comme dans la maquette (cercle à cocher, texte, échéance à droite, en jaune si elle tombe dans les 2 h). Les rappels du jour apparaissent comme points jaunes sur la frise.

- [ ] **Step 1 :** Tests Rust :
  - `take_due` ne renvoie un rappel qu'une seule fois, même en deux appels successifs ;
  - un rappel échu pendant l'absence → au démarrage, `take_due` le renvoie avec `is_late = true`, et il n'est jamais renvoyé ensuite (Review Focus 1) ;
  - `next_due` ignore les rappels faits, supprimés et déjà notifiés ;
  - `reminders.create` avec « demain » → échéance le lendemain à 9 h ;
  - complete puis undo.
- [ ] **Step 2 :** Test TS : cocher appelle `reminders_complete(id, true)` ; une échéance dans moins de 2 h a la classe `soon`.
- [ ] **Step 3 :** **RED**, puis **GREEN**. Commit : `feat(rappels): ajoute les rappels avec notification à l'heure dite`

---

### Task 6 : Notes rapides

**Files :** Create `crates/core/src/notes/{mod.rs,tools.rs}`, `src-tauri/src/modules/notes.rs`, `src/modules/notes/{api.ts,NotesCard.tsx}`. Modify `db.rs` (migration 5), `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`, `Panel.tsx`. Test : fichiers core, `NotesCard.test.tsx`.

**Interfaces :**
- Migration 5 :
  - `notes(id INTEGER PRIMARY KEY, body TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, deleted INTEGER NOT NULL DEFAULT 0, created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL) STRICT` ;
  - une table virtuelle `notes_fts USING fts5(body, content='notes', content_rowid='id', tokenize='unicode61 remove_diacritics 2')` ;
  - les triggers d'insertion, de mise à jour et de suppression qui la synchronisent.
- Core : `insert`, `update`, `set_deleted`, `set_pinned`, `recent(c, limit)` (épinglées d'abord, puis par `updated_ms`), et `search(c, query, limit)`. La recherche est préfixe sur chaque mot, avec `MATCH` et les mots échappés entre guillemets. Une requête vide renvoie `recent`.
- Outils :
  - `notes.create`, Free, `{ text }`. Undo : suppression ;
  - `notes.search`, Free, `{ query }` ;
  - `notes.delete`, Free, `{ id }`. Undo : restauration ;
  - `notes.to_reminder`, Free, `{ id }`. Crée un rappel à partir du texte de la note, via la fonction core des rappels. Undo : supprime ce rappel.
- Front : `NotesCard` (emplacement `notes`) comprend :
  - un champ de saisie (Entrée enregistre, Maj+Entrée va à la ligne) ;
  - une liste des notes récentes, chacune tronquée à 3 lignes, avec au survol les boutons épingler, en rappel et supprimer ;
  - un champ de recherche dans l'en-tête de la carte, qui interroge à la frappe avec un délai de 150 ms.

- [ ] **Step 1 :** Tests Rust :
  - la recherche « reunion » trouve « Réunion client » ;
  - la recherche « cli » trouve « client » ;
  - une note supprimée est absente des recherches ;
  - les caractères spéciaux FTS (`"`, `*`, `-`) ne font pas d'erreur SQL ;
  - `to_reminder` puis undo.
- [ ] **Step 2 :** Tests TS : Entrée enregistre, Maj+Entrée ne le fait pas ; la recherche est temporisée.
- [ ] **Step 3 :** **RED**, puis **GREEN**. Commit : `feat(notes): ajoute les notes rapides et leur recherche`

---

### Task 7 : Minuteurs et focus

**Files :** Create `crates/core/src/timers/{mod.rs,tools.rs}`, `src-tauri/src/modules/timers.rs`, `src/modules/timers/{api.ts,TimerCard.tsx,PillTimer.tsx}`. Modify `db.rs` (migration 6), `settings.rs` (+ `settings.ts`), `state.rs`, `lib.rs`, `build.rs`, `capabilities/{panel,pill}.json`, `Panel.tsx`, `src/pill/Pill.tsx`. Test : fichiers core, `TimerCard.test.tsx`, `PillTimer.test.tsx`.

**Interfaces :**
- Migration 6 : `timers(id INTEGER PRIMARY KEY, label TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('timer','focus','break')), duration_ms INTEGER NOT NULL, ends_ms INTEGER NOT NULL, state TEXT NOT NULL CHECK (state IN ('running','done','cancelled')), focus_cycle INTEGER, created_ms INTEGER NOT NULL) STRICT`.
- Réglages : `focus_minutes: u16` (25, borné 5..=120) et `break_minutes: u16` (5, borné 1..=60). Ils sont ajoutés à `Settings`, à `validate` / `fix` et au miroir TypeScript.
- Core :
  - `start(c, label, kind, duration_ms, now) -> Timer`, `cancel(c, id)`, `running(c) -> Vec<Timer>` ;
  - `take_finished(c, now) -> Vec<Timer>`, qui passe les minuteurs à l'état `done` dans une transaction (une seule fois) ;
  - `next_end(c) -> Option<i64>` ;
  - `pub fn parse_duration(text: &str) -> Option<u64>` comprend « 25 minutes », « 1h30 », « 90 s » et « 10 min ».
- Focus :
  - à la fin d'un `focus`, une pause (`break`) démarre automatiquement ;
  - à la fin d'une pause, rien ne démarre : une notification « Pause terminée » part.
- Outils :
  - `timer.start`, Free, `{ text }` (par exemple « minuteur 25 minutes thé »). Undo : annulation ;
  - `timer.cancel`, Free, `{ id }` ;
  - `focus.start`, Free, `{}`. Utilise `focus_minutes`.

  Les outils signalent `Topic::Timers` et reprogramment l'échéance.
- Tauri :
  - au démarrage, `take_finished(now)` fait partir « Terminé pendant ton absence : {label} » (Review Focus 5) ;
  - les minuteurs encore en cours sont reprogrammés ;
  - `on_due("timers")` envoie la notification « {label} terminé » et enchaîne focus puis pause ;
  - commandes `timers_list`, `timers_start(text)`, `focus_start()` et `timers_cancel(id)`, accessibles au panneau ;
  - `timers_list` est aussi autorisée à la **pilule**.
- Front :
  - `TimerCard` (emplacement `timer`) :
    - anneau de progression du premier minuteur ;
    - liste des autres ;
    - boutons rapides « 5 min », « 25 min » et « Focus » ;
    - champ libre.
  - `PillTimer` affiche `mm:ss` dans la pilule seulement s'il y a un minuteur en cours.
    - mise à jour d'une seconde réservée à ce cas, alignée sur la seconde ;
    - aucune minuterie active sinon (test avec `vi.getTimerCount()`).

- [ ] **Step 1 :** Tests Rust :
  - `parse_duration` ;
  - `take_finished` une seule fois ;
  - minuteur écoulé pendant l'absence → renvoyé une seule fois au démarrage ;
  - un focus terminé démarre une pause, une pause terminée ne démarre rien ;
  - `next_end`.
- [ ] **Step 2 :** Tests TS :
  - `PillTimer` sans minuteur → aucune minuterie active ;
  - avec un minuteur → affiche `24:59` après une seconde ;
  - bouton « 25 min » → `timers_start("25 minutes")`.
- [ ] **Step 3 :** **RED**, puis **GREEN**. Commit : `feat(minuteur): ajoute les minuteurs, le focus et le compte à rebours de la pilule`

---

### Task 8 : Historique du presse-papiers

**Files :** Create `crates/core/src/clipboard/{mod.rs,filter.rs,tools.rs}`, `src-tauri/src/modules/clipboard.rs`, `src/modules/clipboard/{api.ts,ClipboardCard.tsx}`. Modify `db.rs` (migration 7), `settings.rs` (+ ts), `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`, `Cargo.toml` (`image`, features `windows`), `Panel.tsx`. Test : fichiers core, `ClipboardCard.test.tsx`.

**Interfaces :**
- Migration 7 : `clipboard(id INTEGER PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('text','image')), text TEXT, truncated INTEGER NOT NULL DEFAULT 0, png BLOB, thumb BLOB, hash TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, created_ms INTEGER NOT NULL) STRICT`, avec l'index unique `ON clipboard(hash)`.
- Réglages : `clipboard_enabled: bool` (true), `clipboard_retention_days: u16` (7, borné 1..=90) et `clipboard_max_items: u16` (200, borné 20..=1000).
- Core :
  - `pub struct ClipFlags { pub exclude_monitor: bool, pub history_allowed: Option<u32>, pub cloud_allowed: Option<u32> }` ;
  - `pub fn should_record(flags: &ClipFlags) -> bool` (`filter.rs`) : faux si `exclude_monitor`, ou si `history_allowed == Some(0)`, ou si `cloud_allowed == Some(0)` ;
  - `pub const MAX_TEXT_BYTES: usize = 65_536` ;
  - `pub fn truncate_text(s: &str) -> (String, bool)`, tronquée sur une frontière de caractère ;
  - `record_text(c, text, now)` et `record_image(c, png, thumb, now)`. Une copie identique (même `hash`) ne crée pas de doublon : elle remonte l'entrée existante en tête (`created_ms = now`) ;
  - `purge(c, now, retention_days, max_items)`, qui ne touche jamais aux entrées épinglées ;
  - `search(c, query, limit)` (`LIKE`, insensible à la casse), `set_pinned`, `delete`, `get(c, id)`.
- Tauri, écoute :
  - un thread qui possède une fenêtre message-only (`HWND_MESSAGE`) et appelle `AddClipboardFormatListener` ;
  - sur `WM_CLIPBOARDUPDATE`, si `clipboard_enabled` : `OpenClipboard`, avec 2 nouvelles tentatives à 50 ms d'intervalle (Review Focus 3) ;
  - lecture des trois formats d'exclusion via `RegisterClipboardFormatW`, puis `should_record` ;
  - `CF_UNICODETEXT` donne un texte. `CF_DIBV5` / `CF_DIB` donnent une image, convertie en PNG, avec une miniature de 160 px de large maximum (`image`). Une image de plus de 20 Mo n'est pas enregistrée ;
  - ensuite `purge` et `Topic::Clipboard`. Purge aussi une fois au démarrage.
- Tauri, commandes :
  - `clipboard_list(query)` ;
  - `clipboard_pin(id, pinned)`, `clipboard_delete(id)` ;
  - `clipboard_paste(id)` :
    1. remet le contenu dans le presse-papiers, en marquant l'écriture avec un format privé `Oniris.Self` que l'écoute ignore, pour qu'elle ne s'enregistre pas elle-même ;
    2. ferme le panneau ;
    3. après 80 ms, injecte Ctrl+V par `SendInput`, marqué `ONIRIS_MARK`, dans la fenêtre au premier plan. C'est du code produit, **jamais exécuté pendant les tests** (R8).
- Outil `clipboard.search`, Free, `{ query }`.
- Front : `ClipboardCard` (emplacement `clipboard`) affiche :
  - un champ de recherche ;
  - une liste d'entrées (texte sur 2 lignes, ou miniature), avec l'heure relative ;
  - un clic qui colle ;
  - au survol, les boutons épingler et supprimer.

  La miniature est servie en `data:image/png;base64`, déjà autorisé par la CSP.

- [ ] **Step 1 :** Tests Rust :
  - `should_record`, pour les trois formats et l'absence de format ;
  - `truncate_text` sur 10 Mo donne 64 Ko au plus, marqué tronqué, sans couper un caractère multi-octet ;
  - une copie en double ne crée qu'une ligne ;
  - `purge` respecte les épinglés, la durée de conservation et le nombre maximal ;
  - la conversion d'un DIB 32 bits de test en PNG donne la bonne largeur et la bonne hauteur (fonction pure `dib_to_png(&[u8]) -> Result<Vec<u8>>`).
- [ ] **Step 2 :** Test TS : un clic appelle `clipboard_paste(id)`, et la recherche est temporisée.
- [ ] **Step 3 :** **RED**, puis **GREEN**. Commit : `feat(presse-papiers): garde l'historique des copies sans les mots de passe`

---

### Task 9 : État du PC

**Files :** Create `crates/core/src/system/{mod.rs,tools.rs}`, `src-tauri/src/modules/system.rs`, `src/modules/system/{api.ts,PcCard.tsx,Gauge.tsx}`. Modify `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`, `Cargo.toml` (`sysinfo`, features `windows`), `Panel.tsx`. Test : fichiers core, `PcCard.test.tsx`, `Gauge.test.tsx`.

**Interfaces :**
- Core : `pub struct SystemStatus { cpu_pct: f32, ram_used_mb: u64, ram_total_mb: u64, gpu_pct: Option<f32>, vram_used_mb: Option<u64>, vram_total_mb: Option<u64>, oniris_mb: u64, local_ai: String, voice: String }` (Serialize).
  - `local_ai` et `voice` valent « non installée » jusqu'aux plans 5 et 6.
  - Fonction pure `pub fn gpu_from_counters(engine_utils: &[(String, f64)]) -> f32`. Elle somme par moteur `engtype_3D` et prend le maximum par instance de moteur, comme le Gestionnaire des tâches.
- Tauri :
  - échantillonneur actif **seulement panneau ouvert** : il démarre sur `panel-state {open:true}`, s'arrête à la fermeture, et émet `oniris://system` (`SystemStatus`) chaque seconde ;
  - CPU et RAM via `sysinfo`, rafraîchis de façon minimale, sans liste complète des processus ;
  - mémoire d'Oniris : somme de la mémoire privée d'`oniris.exe` et de ses `msedgewebview2.exe` descendants, recalculée toutes les 5 s seulement ;
  - GPU via les compteurs PDH `\GPU Engine(*)\Utilization Percentage` et `\GPU Adapter Memory(*)\Dedicated Usage`. Si PDH échoue, les champs valent `None` et la carte masque ces jauges ;
  - outil `system.status`, Free : un instantané à la demande ;
  - commande `system_snapshot()`.
- Front :
  - `PcCard` (emplacement `pc`) : quatre jauges circulaires CPU, GPU, RAM et VRAM, comme dans la maquette, puis la liste « IA locale », « Voix (Whisper) » et « Assistant (X Mo) » ;
  - `Gauge` : anneau SVG dont le `stroke-dashoffset` passe par une transition CSS.

- [ ] **Step 1 :** Tests Rust : `gpu_from_counters` sur un échantillon de noms d'instances réels (`pid_1234_luid_..._engtype_3D`) ; `SystemStatus` sérialisé avec `gpu_pct: null` quand c'est indisponible.
- [ ] **Step 2 :** Tests TS : `Gauge` à 44 % donne un `dashoffset` proportionnel ; `PcCard` masque GPU et VRAM quand ils valent `null` ; aucun abonnement quand le panneau est fermé.
- [ ] **Step 3 :** **RED**, puis **GREEN**. Commit : `feat(systeme): affiche l'état du PC panneau ouvert seulement`

---

### Task 10 : Météo (Open-Meteo)

**Files :** Create `crates/core/src/kv.rs`, `crates/core/src/weather/{mod.rs,codes.rs,tools.rs}`, `src-tauri/src/modules/weather.rs`, `src/modules/weather/{api.ts,WeatherBadge.tsx}`, `docs/legal/confidentialite.md`. Modify `db.rs` (migration 8), `settings.rs` (+ ts), `state.rs`, `lib.rs`, `build.rs`, `capabilities/panel.json`, `Cargo.toml` (`reqwest`), `src/panel/Header.tsx`. Test : fichiers core avec fixtures JSON dans `crates/core/tests/fixtures/weather/`, `WeatherBadge.test.tsx`.

**Interfaces :**
- Migration 8 : `kv(key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_ms INTEGER NOT NULL) STRICT` ; fonctions `kv::get(c, key) -> Option<(String, i64)>` et `kv::set(c, key, value, now)`.
- Réglage : `weather_city: String` (vide par défaut, 80 caractères maximum). Ville vide = météo désactivée, sans aucune requête.
- Core :
  - `pub struct Place { name, lat: f64, lon: f64 }` ;
  - `pub struct Weather { place: String, temp_c: f32, code: u8, label: String, icon: String, today_min: f32, today_max: f32, rain_from_hour: Option<u8>, fetched_ms: i64 }` ;
  - fonctions pures `parse_geocoding(json) -> Result<Option<Place>>` et `parse_forecast(json, place, now) -> Result<Weather>`. `rain_from_hour` est la première heure des 12 prochaines où `precipitation_probability >= 60` ;
  - `codes::label_icon(code) -> (&str, &str)` traduit les codes WMO en libellés français et en noms d'icônes Lucide ;
  - client `async fn fetch(client: &reqwest::Client, city: &str, cached_place: Option<Place>) -> Result<Weather>`. Il géocode seulement si la ville a changé. URL de géocodage : `https://geocoding-api.open-meteo.com/v1/search?name={city}&count=1&language=fr`. URL de prévision : `https://api.open-meteo.com/v1/forecast?latitude=..&longitude=..&current=temperature_2m,weather_code&hourly=precipitation_probability&daily=temperature_2m_min,temperature_2m_max&timezone=auto&forecast_days=1`. Délai maximal 8 s.
- Tauri, actualisation sans jamais bloquer l'ouverture :
  - à l'ouverture du panneau, si la ville n'est pas vide et que la dernière donnée a plus de 30 min, une actualisation est lancée en tâche de fond ;
  - en cas d'échec, pas de nouvel essai avant 10 min ; la dernière valeur reste servie avec son heure (Review Focus 4).
  - stockage dans `kv` : `weather.last` et `weather.place` ;
  - commandes `weather_get() -> Option<Weather>` (cache uniquement, instantanée) et `weather_refresh()` ;
  - outil `weather.today`, Free, qui renvoie le cache, pour IRIS au plan 5 ;
  - `Topic::Weather` signalé à chaque mise à jour.
- Front : `WeatherBadge` est placé sous l'horloge de l'en-tête, comme dans la maquette (« ☁︎ 12° · pluie vers 18 h · Paris »).
  - Ville vide : un lien « Choisir une ville » ouvre un petit champ en ligne qui enregistre `weather_city` par `settings_save`.
  - Donnée plus vieille que 3 h : affichée en grisé avec l'heure de la mesure.
- `docs/legal/confidentialite.md` : première version. Elle décrit qu'Oniris stocke tout localement et que la météo transmet le nom de la ville et les coordonnées à Open-Meteo (open-meteo.com, sans compte ni clé), seulement si une ville est réglée. Elle annonce aussi les flux des plans suivants comme « à venir », sans les détailler.

- [ ] **Step 1 :** Tests Rust sur des fixtures enregistrées à la main au format documenté d'Open-Meteo :
  - `parse_geocoding` : une ville trouvée, et la réponse sans `results` donne `None` ;
  - `parse_forecast` : température, code et `rain_from_hour` ;
  - JSON malformé → erreur, sans panique ;
  - `label_icon(61)` donne « Pluie faible ».
  - La politique de nouvel essai est une fonction pure `should_refresh(last_ok_ms, last_fail_ms, now) -> bool`, testée sur ses cas limites (30 min et 10 min).
- [ ] **Step 2 :** Tests TS : ville vide → « Choisir une ville » ; donnée de 4 h → classe `stale` ; pluie → « pluie vers 18 h ».
- [ ] **Step 3 :** **RED**, puis **GREEN**. Aucune requête réseau réelle dans les tests. Commit : `feat(meteo): affiche la météo d'Open-Meteo et documente les données transmises`

---

### Task 11 : Intégration du panneau et aperçu

**Files :** Modify `src/panel/Panel.tsx`, `src/panel/panel.css`, `src/panel/CardGrid.tsx`, `src/pill/Pill.tsx`. Test : `src/panel/Panel.test.tsx`.

- [ ] **Step 1 :** Brancher toutes les cartes dans `CardGrid slots={{ agenda, pc, notes, clipboard, timer }}`. `music`, `mails`, `projects` et `vps` restent « Bientôt ».
  - Le panneau défile en interne si la hauteur de l'écran ne suffit pas (`max-height: calc(100vh - 28px)`).
  - La troisième rangée reprend le style des cartes de la maquette.
- [ ] **Step 2 :** Tests :
  - le panneau affiche les titres des 9 emplacements dans l'ordre ;
  - panneau fermé, aucune minuterie active ni aucun appel IPC hormis les abonnements ;
  - à l'ouverture, chaque `useTopic` charge **après** le premier rendu : l'appel IPC part dans un `useEffect`.
- [ ] **Step 3 :** Aperçu headless à 2560×1440, avec des données factices fournies hors Tauri par `api.ts`, uniquement quand `!isTauri`. Capture dans le scratchpad, comparée à la maquette ; ajuster les espacements.
- [ ] **Step 4 :** Lancer `pnpm test`, `pnpm build`, `cargo test --workspace` et `cargo clippy --workspace --all-targets -- -D warnings`. Commit : `feat(panneau): intègre les modules locaux au panneau`

---

### Task 12 : Liste de vérification utilisateur

**Files :** Create `docs/verifications/plan-02-modules-locaux.md`. Modify `docs/superpowers/plans/2026-11-03-oniris-feuille-de-route.md`.

- [ ] **Step 1 :** Rédiger la liste au même format que celle du plan 1. Elle couvre :
  - un événement créé par texte (« demain 14h-15h dentiste ») ;
  - un rappel dans 2 minutes, qui notifie ;
  - un rappel échu pendant que l'app est fermée, notifié « en retard » une seule fois ;
  - les notes et leur recherche avec accents ;
  - un minuteur de 1 min, son affichage dans la pilule, et l'app quittée puis relancée ;
  - le focus suivi de la pause ;
  - le presse-papiers : un texte, une image, une copie depuis 1Password (non enregistrée), et le collage en un clic dans le Bloc-notes, que **l'utilisateur ouvre lui-même** ;
  - l'état du PC, avec les jauges GPU et VRAM sur la RX 7800 XT ;
  - la météo : choix d'une ville, puis comportement hors ligne ;
  - le contenu de `%APPDATA%\Oniris\logs` ;
  - une nouvelle mesure de performance (`tools/perf`) pour vérifier qu'il n'y a pas de régression du CPU au repos avec l'écoute du presse-papiers.
- [ ] **Step 2 :** Cocher le plan 2 dans la feuille de route. Commit : `docs(verifications): liste les contrôles manuels du plan 2`

---

## Fin du plan 2 : critères

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm test` et `pnpm build` passent.
- Les migrations 3 à 8 sont contiguës, testées et idempotentes.
- Tous les outils listés sont enregistrés. `agenda.delete_event` est le seul à exiger une confirmation, et chaque outil qui modifie des données a un `undo` testé.
- Panneau fermé, il n'y a ni minuterie ni réveil, en dehors des échéances programmées et de l'écoute du presse-papiers.
- `docs/legal/confidentialite.md` existe et décrit le flux météo.
- Graphe graphify mis à jour (`--update`).
