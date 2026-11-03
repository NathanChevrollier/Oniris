# Oniris — Plan 1 : Fondations

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Livrer le squelette d'Oniris : la pilule et le panneau (fenêtres, raccourci appui court / maintien, masquage en plein écran), le système visuel tiré de la maquette, le stockage SQLite, les paramètres, le courtier d'outils (confirmations, provenance, journal, annulation), puis mesurer l'ouverture, la RAM et le CPU.

**Architecture :** Application Tauri 2 avec deux fenêtres WebView2 créées au démarrage et jamais détruites. La **pilule** est une petite fenêtre redimensionnée à la taille de son contenu. Le **panneau** couvre tout l'écran principal en transparence : son voile capte les clics « en dehors », et il est simplement masqué ou affiché, jamais rechargé. Le cœur Rust est réparti dans un workspace Cargo : `crates/core` (base, paramètres, courtier d'outils) est sans dépendance à Tauri et se teste seul ; `src-tauri` gère tout ce qui est fenêtres et Windows. L'interface est en React + TypeScript (Vite multi-pages), avec des animations CSS limitées à `transform` et `opacity`.

**Tech Stack :** Tauri 2 (plugins single-instance, autostart, notification, tray-icon), Rust 1.98, crate `windows` 0.62, `rusqlite` (feature `bundled`), `serde` / `serde_json`, `async-trait`, `tokio`, `thiserror`, `parking_lot`, `tempfile` (tests) ; React 19, TypeScript, Vite 8, Vitest 5, Testing Library, Zustand 5. Les versions sont alignées sur MacroWorld (`Projets/MacroWorld/package.json`, `src-tauri/Cargo.toml`).

**Spec :** [`docs/superpowers/specs/2026-11-03-oniris-design.md`](../specs/2026-11-03-oniris-design.md) · Feuille de route : [`2026-11-03-oniris-feuille-de-route.md`](2026-11-03-oniris-feuille-de-route.md) · Maquette de référence : [`docs/design/maquette-panneau.html`](../../design/maquette-panneau.html)

## Global Constraints

- Windows 11 uniquement ; aucun code multiplateforme à prévoir.
- Nom produit `Oniris`, identifiant Tauri `dev.oniris.app`, données dans `%APPDATA%\Oniris` (chemin fixe, indépendant de l'identifiant), base `%APPDATA%\Oniris\oniris.db`.
- Budgets (spec §9) : ouverture du panneau < 100 ms ; CPU au repos panneau fermé ≈ 0 % (< 0,5 % en moyenne) ; RAM panneau fermé aussi basse que possible **sans jamais dégrader l'ouverture**, repère < 200 Mo ; VRAM 0 Go.
- L'ouverture instantanée est impérative et prime sur la RAM.
- Raccourci par défaut **Alt+Espace** : appui court = ouvrir / fermer le panneau ; maintien = push-to-talk. Seuil par défaut : 250 ms.
- Pilule : écran principal, centrée en haut, masquée quand une app est en plein écran (réglable).
- Accent `#ff7448` ; police `"Segoe UI Variable Display", "Segoe UI Variable Text", system-ui` ; `prefers-reduced-motion` respecté.
- SQLite : mode WAL, `foreign_keys=ON`, tables `STRICT`, migrations versionnées par `PRAGMA user_version`, **un seul processus propriétaire de la base**.
- Une action d'outil demande confirmation si l'outil est marqué `Confirm` **ou** si un argument a une provenance non fiable. Cette règle vit dans le courtier, jamais dans un modèle.
- Aucun identifiant ni secret personnel dans le code ; prénom de l'utilisateur stocké dans les paramètres (vide par défaut).
- Avant le premier `pnpm install` : un `pnpm-workspace.yaml` local (sinon `~/pnpm-workspace.yaml` capture le projet).
- Commits Conventional Commits en français, sans co-signature ; ni commit ni push sans l'accord de l'utilisateur ; jamais de `pnpm tauri build` sans son accord.

## Review Focus

1. **Écran à 150 % de mise à l'échelle, ou écran principal qui n'est pas à l'origine (0,0)** → la pilule reste centrée en haut de l'écran principal et le panneau le couvre exactement. Test dans la tâche 3.
2. **Alt+Espace maintenu : la répétition automatique du clavier envoie plusieurs « touche enfoncée »** → un seul début de push-to-talk, aucune bascule parasite du panneau. Test dans la tâche 4.
3. **Relâchement d'Alt après le raccourci** → aucune activation du menu de l'application au premier plan, et le menu système de fenêtre (comportement natif d'Alt+Espace) ne s'ouvre pas. Vérification manuelle dans la tâche 4.
4. **Paramètres enregistrés par une version plus ancienne (champs manquants) ou JSON corrompu** → valeurs par défaut pour les champs manquants ; si le JSON est illisible, on part des valeurs par défaut sans planter, et l'ancien JSON est sauvegardé. Test dans la tâche 6.
5. **Action en attente de confirmation jamais tranchée, ou app redémarrée entre-temps** → elle expire après 10 min et ne s'exécute jamais plus tard. Tests dans la tâche 7.

---

## Arborescence créée par ce plan

```
Oniris/
  Cargo.toml                    workspace : members = ["src-tauri", "crates/core"]
  package.json  pnpm-workspace.yaml  tsconfig.json  vite.config.ts  .gitignore
  pill.html  panel.html         points d'entrée Vite (multi-pages)
  src/
    ui/tokens.css  ui/glass.css  ui/motion.css      système visuel (tâche 2)
    ui/Orb.tsx  ui/Card.tsx                          composants de base (tâche 2)
    lib/ipc.ts  lib/time.ts  lib/settings.ts         pont typé vers Rust + utilitaires
    pill/main.tsx  pill/Pill.tsx
    panel/main.tsx  panel/Panel.tsx  panel/Header.tsx  panel/Timeline.tsx
    panel/CommandBar.tsx  panel/CardGrid.tsx  panel/ConfirmCard.tsx
  crates/core/src/
    lib.rs  error.rs  db.rs  settings.rs
    tools/mod.rs  tools/broker.rs  tools/journal.rs
  src-tauri/src/
    main.rs  lib.rs  state.rs  commands.rs  tray.rs  perf.rs
    windows/mod.rs  windows/geometry.rs
    hotkey/mod.rs  hotkey/classifier.rs  hotkey/hook.rs
    fullscreen.rs
  tools/perf/measure.mjs  tools/perf/README.md
```

---

### Task 1 : Squelette du dépôt et des deux fenêtres

**Files :**
- Create : `Cargo.toml`, `package.json`, `pnpm-workspace.yaml`, `tsconfig.json`, `vite.config.ts`, `.gitignore`, `pill.html`, `panel.html`, `src/pill/main.tsx`, `src/panel/main.tsx`, `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/error.rs`
- Test : `crates/core/src/lib.rs` (test fumée), `src/lib/time.test.ts` (créé à la tâche 8 ; ici, Vitest doit seulement se lancer)

**Interfaces :**
- Produces : fenêtres Tauri `pill` et `panel` (labels exacts) ; package Rust `oniris` (lib `oniris_lib`) ; package `oniris-core` ; `pub enum CoreError` (thiserror) avec les variantes `Db(rusqlite::Error)`, `Json(serde_json::Error)`, `Io(std::io::Error)`, `NotFound(String)`, `Invalid(String)`.

- [ ] **Step 1 : Demander l'accord pour `git init`** (le dossier n'est pas versionné). Sans accord, continuer sans git et sauter les étapes de commit.
- [ ] **Step 2 : Créer `pnpm-workspace.yaml`** (`packages: ["."]`, `allowBuilds: { esbuild: true }`) **avant tout `pnpm install`**, puis `package.json` (scripts `dev`, `build` = `tsc --noEmit && vite build`, `test` = `vitest run`, `tauri`) et installer les dépendances listées dans la Tech Stack avec `pnpm add`.
- [ ] **Step 3 : Configurer Vite en multi-pages** : `build.rollupOptions.input = { pill: "pill.html", panel: "panel.html" }`, `server.port = 1420`, `strictPort: true` ; Vitest avec `environment: "jsdom"`.
- [ ] **Step 4 : Créer le workspace Cargo** (`resolver = "3"`, `[profile.release]` : `lto = true`, `codegen-units = 1`, `strip = true`), `crates/core` avec `CoreError`, et `src-tauri` avec ses plugins (`tauri` feature `tray-icon`, `tauri-plugin-single-instance`, `-autostart`, `-notification`).
- [ ] **Step 5 : Déclarer les deux fenêtres dans `tauri.conf.json`** :
  - `pill` : `url: "pill.html"`, `transparent`, `decorations: false`, `alwaysOnTop`, `skipTaskbar`, `resizable: false`, `shadow: false`, `focus: false`, `visible: false`, 420×48 ;
  - `panel` : `url: "panel.html"`, mêmes options sauf `focus: true`, `visible: false`, 1280×800.

  Ajouter `"app.windows[].backgroundColor"` transparent, et une CSP `default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:`.
- [ ] **Step 6 : Écrire un test fumée** `core_error_displays_message` : `CoreError::Invalid("x".into()).to_string()` contient `"x"`.
- [ ] **Step 7 : Vérifier**. Lancer `cargo test --workspace` (attendu : PASS), puis `pnpm test` (attendu : « No test files found » ou PASS), puis `pnpm tauri dev` (attendu : le processus démarre sans erreur, les deux fenêtres existent et restent invisibles).
- [ ] **Step 8 : Commit** (si autorisé) : `chore(socle): initialise le projet Tauri avec les fenêtres pilule et panneau`

---

### Task 2 : Système visuel et composants de base

**Files :**
- Create : `src/ui/tokens.css`, `src/ui/glass.css`, `src/ui/motion.css`, `src/ui/Orb.tsx`, `src/ui/Card.tsx`
- Test : `src/ui/Orb.test.tsx`, `src/ui/Card.test.tsx`

**Interfaces :**
- Produces :
  - `type OrbState = "idle" | "listening" | "thinking" | "speaking"` ;
  - `<Orb state={OrbState} size={number} level?={number} />`, qui rend `data-state={state}` et une variable CSS `--level` (0..1, niveau de la voix) ;
  - `<Card title={string} aside?={ReactNode} span={3|4|5|6|12} delay?={number}>` ;
  - les variables CSS de `tokens.css` reprises **à l'identique** de `:root` dans `docs/design/maquette-panneau.html` (`--txt`, `--txt2`, `--txt3`, `--glass`, `--card`, `--card-h`, `--line`, `--line2`, `--acc`, `--acc2`, `--blue`, `--green`, `--yellow`, `--purple`, `--teal`, `--ease`, `--font`).

- [ ] **Step 1 : Écrire les tests qui échouent**

```tsx
it("Orb expose son état et son niveau", () => {
  const { container } = render(<Orb state="listening" size={58} level={0.4} />);
  const el = container.firstElementChild as HTMLElement;
  expect(el.dataset.state).toBe("listening");
  expect(el.style.getPropertyValue("--level")).toBe("0.4");
});
it("Card affiche le titre en majuscules via CSS, pas dans le texte", () => {
  render(<Card title="Mails" span={5}>x</Card>);
  expect(screen.getByRole("heading", { name: "Mails" })).toBeInTheDocument();
});
```

- [ ] **Step 2 : Lancer** `pnpm test src/ui`. Attendu : FAIL (modules introuvables).
- [ ] **Step 3 : Implémenter**. Porter les règles `.orb`, `.card`, `.gl` et les `@keyframes` (`spin`, `breathe`, `rise`, `eq`) de la maquette dans les trois fichiers CSS. Chaque état de l'Orb modifie la vitesse de rotation et l'amplitude du halo via `[data-state=…]`. `--level` module l'échelle du halo. `@media (prefers-reduced-motion: reduce)` coupe toutes les animations.
- [ ] **Step 4 : Lancer** `pnpm test src/ui`. Attendu : PASS.
- [ ] **Step 5 : Commit** : `feat(ui): ajoute le système visuel et les composants Orb et Card`

---

### Task 3 : Placement des fenêtres et ouverture / fermeture du panneau

**Files :**
- Create : `src-tauri/src/windows/geometry.rs`, `src-tauri/src/windows/mod.rs`, `src-tauri/src/state.rs`, `src-tauri/src/commands.rs`, `src/lib/ipc.ts`
- Modify : `src-tauri/src/lib.rs`
- Test : `src-tauri/src/windows/geometry.rs` (`#[cfg(test)]`)

**Interfaces :**
- Produces (Rust) :
  - `pub struct Rect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }` (pixels physiques) ;
  - `pub fn pill_rect(monitor: Rect, scale: f64, content_width_logical: f64) -> Rect` : hauteur 48 logiques, marge haute 0, centrée sur `monitor`, largeur bornée entre 160 et 640 logiques ;
  - `pub fn panel_rect(monitor: Rect) -> Rect` : renvoie l'écran entier ;
  - `pub fn open_panel(app: &AppHandle) -> tauri::Result<()>`, `pub fn close_panel(app: &AppHandle) -> tauri::Result<()>`, `pub fn toggle_panel(app: &AppHandle) -> tauri::Result<()>`. Chacune met à jour `AppState.panel_open` et émet l'événement `oniris://panel-state` avec la charge utile `{ open: bool, at_ms: u64 }` (`at_ms` = `SystemTime` en ms depuis l'époque Unix, au moment de `show()`/`hide()`) ;
  - commandes Tauri `panel_open`, `panel_close`, `panel_toggle`, `pill_resize(width: f64)`.
- Produces (TS, `src/lib/ipc.ts`) : `ipc.panelOpen()`, `ipc.panelClose()`, `ipc.panelToggle()`, `ipc.pillResize(width: number)`, `onPanelState(cb: (p: { open: boolean; at_ms: number }) => void): Promise<UnlistenFn>`.

- [ ] **Step 1 : Écrire les tests qui échouent**

```rust
#[test] fn pill_centree_en_haut_a_100_pourcent() {
    let m = Rect { x: 0, y: 0, w: 2560, h: 1440 };
    assert_eq!(pill_rect(m, 1.0, 400.0), Rect { x: 1080, y: 0, w: 400, h: 48 });
}
#[test] fn pill_a_150_pourcent_sur_ecran_decale() {
    let m = Rect { x: -1920, y: 0, w: 1920, h: 1080 };
    assert_eq!(pill_rect(m, 1.5, 400.0), Rect { x: -1920 + 660, y: 0, w: 600, h: 72 });
}
#[test] fn pill_largeur_bornee() {
    let m = Rect { x: 0, y: 0, w: 2560, h: 1440 };
    assert_eq!(pill_rect(m, 1.0, 5000.0).w, 640);
    assert_eq!(pill_rect(m, 1.0, 10.0).w, 160);
}
#[test] fn panel_couvre_l_ecran() {
    let m = Rect { x: -1920, y: 0, w: 1920, h: 1080 };
    assert_eq!(panel_rect(m), m);
}
```

- [ ] **Step 2 : Lancer** `cargo test -p oniris windows::geometry`. Attendu : FAIL.
- [ ] **Step 3 : Implémenter `geometry.rs`**, puis `windows/mod.rs`. Récupérer l'écran principal via `app.primary_monitor()`. `open_panel` place et affiche le panneau, appelle `set_focus()` et émet l'événement ; `close_panel` le masque. Le panneau n'est **jamais** détruit ni rechargé. La pilule s'affiche au démarrage.
- [ ] **Step 4 : Lancer** `cargo test -p oniris windows`. Attendu : PASS.
- [ ] **Step 5 : Vérifier à la main** avec `pnpm tauri dev`. Dans la console du panneau, `__TAURI__.core.invoke("panel_toggle")` doit l'afficher sur tout l'écran principal ; un second appel doit le masquer.
- [ ] **Step 6 : Commit** : `feat(fenetres): place la pilule et ouvre ou ferme le panneau`

---

### Task 4 : Raccourci global (appui court / maintien)

**Files :**
- Create : `src-tauri/src/hotkey/classifier.rs`, `src-tauri/src/hotkey/hook.rs`, `src-tauri/src/hotkey/mod.rs`
- Modify : `src-tauri/src/lib.rs`
- Test : `src-tauri/src/hotkey/classifier.rs` (`#[cfg(test)]`)

**Interfaces :**
- Consumes : `toggle_panel(app)` (tâche 3), `Settings.hotkey` et `Settings.hold_threshold_ms` (tâche 6). Avant la tâche 6, utiliser Alt+Espace et 250 ms codés comme valeurs initiales de `HotkeyConfig::default()`.
- Produces :
  - `pub enum KeyEvent { ChordDown { at_ms: u64 }, ChordUp { at_ms: u64 }, Tick { at_ms: u64 } }` ;
  - `pub enum HotkeyAction { TogglePanel, PttStart, PttEnd }` ;
  - `pub struct Classifier`, avec `Classifier::new(threshold_ms: u64)` et `fn feed(&mut self, ev: KeyEvent) -> Option<HotkeyAction>` ;
  - `pub struct HotkeyConfig { pub alt: bool, pub ctrl: bool, pub shift: bool, pub win: bool, pub vk: u16, pub threshold_ms: u64 }` ;
  - `pub fn start_hook(app: AppHandle, cfg: Arc<RwLock<HotkeyConfig>>)` ;
  - l'événement `oniris://ptt`, avec la charge utile `{ phase: "start" | "end", at_ms: u64 }`.

- [ ] **Step 1 : Écrire les tests qui échouent**

```rust
#[test] fn appui_court_bascule_au_relachement() {
    let mut c = Classifier::new(250);
    assert_eq!(c.feed(KeyEvent::ChordDown { at_ms: 0 }), None);
    assert_eq!(c.feed(KeyEvent::ChordUp { at_ms: 120 }), Some(HotkeyAction::TogglePanel));
}
#[test] fn maintien_demarre_puis_termine_le_ptt() {
    let mut c = Classifier::new(250);
    c.feed(KeyEvent::ChordDown { at_ms: 0 });
    assert_eq!(c.feed(KeyEvent::Tick { at_ms: 249 }), None);
    assert_eq!(c.feed(KeyEvent::Tick { at_ms: 250 }), Some(HotkeyAction::PttStart));
    assert_eq!(c.feed(KeyEvent::Tick { at_ms: 400 }), None);
    assert_eq!(c.feed(KeyEvent::ChordUp { at_ms: 900 }), Some(HotkeyAction::PttEnd));
}
#[test] fn repetition_automatique_ignoree() {
    let mut c = Classifier::new(250);
    c.feed(KeyEvent::ChordDown { at_ms: 0 });
    assert_eq!(c.feed(KeyEvent::ChordDown { at_ms: 30 }), None);
    assert_eq!(c.feed(KeyEvent::ChordDown { at_ms: 60 }), None);
    assert_eq!(c.feed(KeyEvent::ChordUp { at_ms: 100 }), Some(HotkeyAction::TogglePanel));
}
#[test] fn relachement_sans_appui_ignore() {
    let mut c = Classifier::new(250);
    assert_eq!(c.feed(KeyEvent::ChordUp { at_ms: 10 }), None);
}
```

- [ ] **Step 2 : Lancer** `cargo test -p oniris hotkey::classifier`. Attendu : FAIL.
- [ ] **Step 3 : Implémenter `Classifier`**. Deux états, `Idle` et `Held { since, ptt_started }`. Les `Tick` ne comptent qu'en `Held`.
- [ ] **Step 4 : Lancer les tests**. Attendu : PASS.
- [ ] **Step 5 : Implémenter `hook.rs`**. Le crochet clavier bas niveau `WH_KEYBOARD_LL` tourne sur un thread dédié qui possède sa boucle de messages, et un `SetTimer` à 40 ms émet des `Tick` tant que le raccourci est enfoncé.
  - La touche du raccourci est **avalée** (le crochet renvoie 1), ce qui empêche l'ouverture du menu système d'Alt+Espace.
  - Avant le relâchement d'Alt, une frappe factice `VK 0xE8` est injectée, marquée dans `dwExtraInfo` par une constante `ONIRIS_MARK` et ignorée par notre propre crochet. Ce « masque de menu » empêche le menu de l'application au premier plan de s'activer.
  - Les actions sont transmises à la boucle principale de Tauri avec `app.run_on_main_thread`.
- [ ] **Step 6 : Vérifier à la main** (Review Focus 2 et 3), avec `pnpm tauri dev` et l'Explorateur Windows au premier plan :
  - un appui court ouvre puis referme le panneau ;
  - un maintien d'une seconde produit exactement un `ptt start` et un `ptt end` (journal de la console) ;
  - après le relâchement, le menu de l'Explorateur n'est pas activé (aucune lettre soulignée) et le menu système ne s'ouvre pas.
- [ ] **Step 7 : Commit** : `feat(raccourci): distingue l'appui court et le maintien d'Alt+Espace`

---

### Task 5 : Détection du plein écran et masquage de la pilule

**Files :**
- Create : `src-tauri/src/fullscreen.rs`
- Modify : `src-tauri/src/lib.rs`
- Test : `src-tauri/src/fullscreen.rs` (`#[cfg(test)]`)

**Interfaces :**
- Consumes : `Rect` (tâche 3).
- Produces :
  - `pub fn is_fullscreen(foreground: Rect, monitor: Rect, is_shell_window: bool, quns_busy: bool) -> bool` ;
  - `pub fn start_watcher(app: AppHandle)` ;
  - l'événement `oniris://fullscreen` avec `{ active: bool }`. Quand `active` est vrai et que `Settings.hide_pill_in_fullscreen` (tâche 6) est activé, la pilule est masquée ; elle réapparaît quand `active` repasse à faux.

- [ ] **Step 1 : Écrire les tests qui échouent**

```rust
const M: Rect = Rect { x: 0, y: 0, w: 2560, h: 1440 };
#[test] fn jeu_plein_ecran() { assert!(is_fullscreen(M, M, false, false)); }
#[test] fn bureau_n_est_pas_plein_ecran() { assert!(!is_fullscreen(M, M, true, false)); }
#[test] fn fenetre_maximisee_n_est_pas_plein_ecran() {
    assert!(!is_fullscreen(Rect { x: 0, y: 0, w: 2560, h: 1392 }, M, false, false));
}
#[test] fn mode_presentation_signale_par_windows() {
    assert!(is_fullscreen(Rect { x: 10, y: 10, w: 800, h: 600 }, M, false, true));
}
```

- [ ] **Step 2 : Lancer** `cargo test -p oniris fullscreen`. Attendu : FAIL.
- [ ] **Step 3 : Implémenter**.
  - `is_shell_window` est vrai pour les classes `Progman`, `WorkerW` et `Shell_TrayWnd`.
  - `quns_busy` est vrai quand `SHQueryUserNotificationState` renvoie `QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN` ou `QUNS_PRESENTATION_MODE`.
  - Le watcher s'appuie sur `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` et `EVENT_OBJECT_LOCATIONCHANGE`, en ne regardant que la fenêtre au premier plan. Il émet seulement quand l'état change. **Pas d'interrogation périodique.**
- [ ] **Step 4 : Lancer les tests**. Attendu : PASS.
- [ ] **Step 5 : Vérifier à la main** : une vidéo YouTube en plein écran (F11) fait disparaître la pilule, et la sortie du plein écran la fait revenir.
- [ ] **Step 6 : Commit** : `feat(pilule): masque la pilule quand une application est en plein écran`

---

### Task 6 : Base SQLite et paramètres

**Files :**
- Create : `crates/core/src/db.rs`, `crates/core/src/settings.rs`, `src/lib/settings.ts`
- Modify : `crates/core/src/lib.rs`, `src-tauri/src/state.rs`, `src-tauri/src/commands.rs`, `src/lib/ipc.ts`
- Test : `crates/core/src/db.rs` et `crates/core/src/settings.rs` (`#[cfg(test)]`, avec `tempfile`)

**Interfaces :**
- Produces (Rust, `oniris_core`) :
  - `pub fn open_db(path: &Path) -> Result<Connection, CoreError>` : WAL, `foreign_keys=ON`, `busy_timeout=5000`, `synchronous=NORMAL` ;
  - `pub struct Migration { pub version: u32, pub sql: &'static str }` ;
  - `pub fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<u32, CoreError>` : applique dans une transaction les migrations de version supérieure à `user_version`, et renvoie la version finale ;
  - `pub const CORE_MIGRATIONS: &[Migration]` : la v1 crée `settings` et `actions` (tâche 7). Les plans suivants ajoutent leurs propres tableaux de migrations, avec des numéros de version uniques.
  - `pub struct Settings`, avec `#[serde(default)]` et ces champs : `user_name: String` (""), `hotkey: HotkeySetting` (Alt+Espace), `hold_threshold_ms: u32` (250), `hover_open: bool` (true), `hover_delay_ms: u32` (350), `autostart: bool` (true), `hide_pill_in_fullscreen: bool` (true), `timeline_start_hour: u8` (8), `timeline_end_hour: u8` (23), `reduce_motion: bool` (false) ;
  - `pub struct HotkeySetting { pub alt: bool, pub ctrl: bool, pub shift: bool, pub win: bool, pub key: String }`, avec `key` au format `"Space"` ou `"A"` à `"Z"`, `"F1"` à `"F12"` ;
  - `pub fn load_settings(conn: &Connection) -> Result<Settings, CoreError>` et `pub fn save_settings(conn: &Connection, s: &Settings) -> Result<(), CoreError>`. La table est `settings(id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL, broken_json TEXT) STRICT`.
- Produces (Tauri / TS) : commandes `settings_get` et `settings_save`, l'événement `oniris://settings` (paramètres complets après chaque enregistrement), et `ipc.getSettings(): Promise<Settings>`, `ipc.saveSettings(s: Settings)`, avec un type `Settings` TypeScript miroir dans `src/lib/settings.ts`.

- [ ] **Step 1 : Écrire les tests qui échouent**

```rust
#[test] fn base_en_wal_et_strict() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = open_db(&dir.path().join("t.db")).unwrap();
    let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    migrate(&mut c, CORE_MIGRATIONS).unwrap();
    assert!(c.execute("INSERT INTO settings(id, json) VALUES ('a', 1)", []).is_err());
}
#[test] fn migrations_idempotentes() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = open_db(&dir.path().join("t.db")).unwrap();
    let v1 = migrate(&mut c, CORE_MIGRATIONS).unwrap();
    let v2 = migrate(&mut c, CORE_MIGRATIONS).unwrap();
    assert_eq!(v1, v2);
}
#[test] fn champs_manquants_remplis_par_defaut() {
    let s: Settings = serde_json::from_str(r#"{"user_name":"Nathan"}"#).unwrap();
    assert_eq!(s.user_name, "Nathan");
    assert_eq!(s.hold_threshold_ms, 250);
    assert_eq!(s.timeline_end_hour, 23);
}
#[test] fn json_corrompu_donne_les_defauts_et_garde_une_copie() {
    // insérer json = "{pas du json" puis load_settings
    // attendu : Settings::default(), et la colonne broken_json contient "{pas du json"
}
```

- [ ] **Step 2 : Lancer** `cargo test -p oniris-core`. Attendu : FAIL.
- [ ] **Step 3 : Implémenter `db.rs`, puis `settings.rs`**. Un JSON illisible est copié dans `broken_json`, puis remplacé par les valeurs par défaut sérialisées. Brancher l'ouverture de la base dans `state.rs` au démarrage (`%APPDATA%\Oniris\oniris.db`, dossier créé si besoin). `AppState.db` est un `Arc<parking_lot::Mutex<Connection>>`, seule et unique connexion du processus. Le raccourci (tâche 4) relit `HotkeyConfig` depuis les paramètres et se met à jour sur `settings_save`.
- [ ] **Step 4 : Lancer** `cargo test --workspace`. Attendu : PASS.
- [ ] **Step 5 : Commit** : `feat(stockage): ajoute la base SQLite et les paramètres`

---

### Task 7 : Courtier d'outils, confirmations et journal

**Files :**
- Create : `crates/core/src/tools/mod.rs`, `crates/core/src/tools/broker.rs`, `crates/core/src/tools/journal.rs`, `src/panel/ConfirmCard.tsx`
- Modify : `crates/core/src/db.rs` (table `actions` dans `CORE_MIGRATIONS` v1), `src-tauri/src/commands.rs`, `src/lib/ipc.ts`
- Test : `crates/core/src/tools/broker.rs` (`#[cfg(test)]`, `#[tokio::test]`), `src/panel/ConfirmCard.test.tsx`

**Interfaces :**
- Produces (Rust, `oniris_core::tools`) :
  - énumérations :
    - `pub enum Permission { Free, Confirm }` ;
    - `pub enum Risk { Low, Medium, High }` ;
    - `pub enum Brain { Local, Claude }` ;
    - `pub enum Origin { User, Assistant { brain: Brain }, Mcp }` ;
    - `pub enum Provenance { User, Untrusted { source: String } }` ;
  - structures :
    - `pub struct ToolSpec { pub name: String, pub description: String, pub args_schema: serde_json::Value, pub permission: Permission }` ;
    - `pub struct ToolCall { pub tool: String, pub args: Value, pub provenance: Provenance, pub origin: Origin }` ;
    - `pub struct ConfirmationText { pub summary: String, pub details: String, pub risk: Risk }` ;
    - `pub struct ToolResult { pub output: Value, pub undo: Option<Value> }` ;
  - `pub enum ToolError { BadArgs(String), Failed(String), NotUndoable, UnknownTool(String), Expired, NotPending }` ;
  - le trait outil :

    ```rust
    #[async_trait]
    pub trait Tool: Send + Sync {
        fn spec(&self) -> ToolSpec;
        fn describe(&self, args: &Value) -> ConfirmationText;
        async fn run(&self, args: &Value) -> Result<ToolResult, ToolError>;
        async fn undo(&self, _undo: &Value) -> Result<(), ToolError> { Err(ToolError::NotUndoable) }
    }
    ```

  - `pub enum ToolOutcome { Done { action_id: i64, output: Value }, NeedsConfirmation(PendingAction), Rejected { action_id: i64 }, Failed { action_id: i64, error: String } }` ;
  - `pub struct PendingAction { pub action_id: i64, pub tool: String, pub text: ConfirmationText, pub provenance: Provenance, pub expires_at_ms: u64 }` ;
  - `pub struct JournalEntry { pub id: i64, pub ts_ms: u64, pub tool: String, pub args: Value, pub origin: Origin, pub provenance: Provenance, pub status: String, pub result: Option<Value>, pub undoable: bool }` ;
  - `pub struct ToolBroker`, avec `new(db: Arc<Mutex<Connection>>, clock: Arc<dyn Fn() -> u64 + Send + Sync>)`, `register(&mut self, tool: Arc<dyn Tool>)`, `specs(&self) -> Vec<ToolSpec>`, `async call(&self, call: ToolCall) -> ToolOutcome`, `async resolve(&self, action_id: i64, approved: bool) -> ToolOutcome`, `async undo(&self, action_id: i64) -> Result<(), ToolError>`, `journal(&self, limit: u32) -> Vec<JournalEntry>`, `expire_stale(&self)`.
  - Constante `PENDING_TTL_MS: u64 = 600_000`.
  - Table `actions(id INTEGER PRIMARY KEY, ts_ms INTEGER NOT NULL, tool TEXT NOT NULL, args TEXT NOT NULL, origin TEXT NOT NULL, provenance TEXT NOT NULL, status TEXT NOT NULL CHECK (status IN ('pending','done','failed','rejected','expired','undone')), result TEXT, undo TEXT, expires_at_ms INTEGER) STRICT`.
- Produces (Tauri / TS) : commandes `tool_resolve(action_id, approved)`, `tool_undo(action_id)` et `journal_list(limit)` ; l'événement `oniris://confirm` (charge utile `PendingAction`) ; `<ConfirmCard pending={PendingAction} onResolve={(approved: boolean) => void} />`, qui affiche le résumé, les détails dans un bloc à chasse fixe, le niveau de risque et, si la provenance n'est pas fiable, la mention « Demandé à partir de : {source} ».

- [ ] **Step 1 : Écrire les tests qui échouent**, avec un outil de test `Echo` paramétrable (permission, undo) :

```rust
#[tokio::test] async fn outil_libre_demande_par_l_utilisateur_s_execute() {
    // Echo Free + Provenance::User → ToolOutcome::Done, journal status "done"
}
#[tokio::test] async fn outil_a_confirmer_reste_en_attente() {
    // Echo Confirm → NeedsConfirmation ; run() non appelé (compteur = 0)
}
#[tokio::test] async fn provenance_non_fiable_force_la_confirmation() {
    // Echo Free + Provenance::Untrusted{source:"mail:abc"} → NeedsConfirmation
}
#[tokio::test] async fn refus_journalise_et_jamais_execute() {
    // NeedsConfirmation puis resolve(id,false) → Rejected ; compteur run = 0 ; status "rejected"
}
#[tokio::test] async fn acceptation_execute_une_seule_fois() {
    // resolve(id,true) → Done ; un second resolve(id,true) → Failed avec ToolError::NotPending ; compteur = 1
}
#[tokio::test] async fn attente_expiree_ne_s_execute_jamais() {
    // horloge simulée : call à t=0, resolve(id,true) à t=PENDING_TTL_MS+1 → Failed(Expired) ; compteur = 0 ; status "expired"
}
#[tokio::test] async fn expiration_au_redemarrage() {
    // insérer une ligne pending dont expires_at_ms est passé, nouveau ToolBroker, expire_stale() → status "expired"
}
#[tokio::test] async fn annulation() {
    // Echo avec undo → Done ; undo(id) → Ok ; status "undone" ; un second undo → Err(NotUndoable)
}
#[tokio::test] async fn outil_inconnu() {
    // call("nope") → Failed avec UnknownTool, journalisé
}
```

```tsx
it("ConfirmCard signale la provenance non fiable", () => {
  render(<ConfirmCard pending={{ action_id: 1, tool: "shell", text: { summary: "Lancer une commande", details: "Remove-Item x", risk: "High" }, provenance: { Untrusted: { source: "mail de inconnu@x.fr" } }, expires_at_ms: 0 }} onResolve={() => {}} />);
  expect(screen.getByText(/Demandé à partir de : mail de inconnu@x.fr/)).toBeInTheDocument();
});
```

- [ ] **Step 2 : Lancer** `cargo test -p oniris-core tools` puis `pnpm test src/panel/ConfirmCard`. Attendu : FAIL.
- [ ] **Step 3 : Implémenter le courtier**. Les arguments, l'origine et la provenance sont sérialisés en JSON dans le journal. Une action « pending » ne peut passer qu'une seule fois à un autre statut, via un `UPDATE … WHERE status = 'pending'` dont on vérifie le nombre de lignes modifiées. `expire_stale()` est appelé au démarrage et avant chaque `resolve`. Le Rust émet `oniris://confirm` ; le panneau s'ouvre s'il est fermé et affiche `ConfirmCard`.
- [ ] **Step 4 : Lancer les tests**. Attendu : PASS.
- [ ] **Step 5 : Commit** : `feat(outils): ajoute le courtier d'outils avec confirmations et journal`

---

### Task 8 : Coquille du panneau et de la pilule

**Files :**
- Create : `src/lib/time.ts`, `src/pill/Pill.tsx`, `src/panel/Panel.tsx`, `src/panel/Header.tsx`, `src/panel/Timeline.tsx`, `src/panel/CommandBar.tsx`, `src/panel/CardGrid.tsx`
- Modify : `src/pill/main.tsx`, `src/panel/main.tsx`
- Test : `src/lib/time.test.ts`, `src/panel/Panel.test.tsx`, `src/pill/Pill.test.tsx`

**Interfaces :**
- Consumes : `Orb`, `Card` (tâche 2) ; `ipc.*`, `onPanelState` (tâche 3) ; `ipc.getSettings` (tâche 6) ; `ConfirmCard` (tâche 7).
- Produces :
  - `nowPosition(now: Date, startHour: number, endHour: number): number`, borné entre 0 et 1 ;
  - `greeting(now: Date, name: string): string` : « Bonjour » avant 18 h, « Bonsoir » ensuite, suivi du prénom s'il n'est pas vide (« Bonjour » seul sinon) ;
  - `formatLongDate(now: Date): string`, par exemple « Lundi 3 novembre » ;
  - `<CardGrid>`, qui reçoit des cartes enfants sur 12 colonnes. Les plans 2 à 4 y branchent leurs cartes ; ici, il affiche des emplacements vides avec la mention « Bientôt » ;
  - `<Timeline startHour endHour items={TimelineItem[]} />` avec `type TimelineItem = { start: Date; end?: Date; label: string; kind: "event" | "reminder"; color?: string }` ;
  - `<CommandBar onSubmit={(text: string) => void} />`, qui prend le focus à chaque ouverture du panneau ;
  - un store Zustand `usePanelStore` avec `{ open: boolean; pending: PendingAction[] }`.

- [ ] **Step 1 : Écrire les tests qui échouent**

```ts
it("nowPosition", () => {
  expect(nowPosition(new Date(2026, 10, 3, 8, 0), 8, 23)).toBe(0);
  expect(nowPosition(new Date(2026, 10, 3, 15, 30), 8, 23)).toBeCloseTo(0.5);
  expect(nowPosition(new Date(2026, 10, 3, 6, 0), 8, 23)).toBe(0);
  expect(nowPosition(new Date(2026, 10, 3, 23, 59), 8, 23)).toBe(1);
});
it("greeting", () => {
  expect(greeting(new Date(2026, 10, 3, 9), "Nathan")).toBe("Bonjour Nathan");
  expect(greeting(new Date(2026, 10, 3, 19), "")).toBe("Bonsoir");
});
it("formatLongDate", () => { expect(formatLongDate(new Date(2026, 10, 3))).toBe("Mardi 3 novembre"); });
```

```tsx
it("Échap ferme le panneau", async () => {
  // mock ipc.panelClose ; render(<Panel />) ; user.keyboard("{Escape}") ; expect(panelClose).toHaveBeenCalledOnce()
});
it("clic sur le voile ferme, clic dans le panneau non", async () => { /* mêmes mocks */ });
it("la pilule demande sa taille au Rust", () => { /* ResizeObserver mocké → ipc.pillResize appelé avec la largeur mesurée */ });
```

- [ ] **Step 2 : Lancer** `pnpm test`. Attendu : FAIL.
- [ ] **Step 3 : Implémenter**, en reprenant la mise en page de la maquette : en-tête (salutation, orbe, horloge), bandeau de briefing (texte fixe « IRIS arrive bientôt. » jusqu'au plan 5), frise, grille et barre de commande. Règles de performance :
  - l'horloge est un composant isolé, mis à jour une fois par seconde **seulement quand le panneau est ouvert**, et elle ne fait pas re-rendre le reste ;
  - toutes les animations s'arrêtent quand le panneau est fermé (classe `closed` sur `<body>`, avec `animation-play-state: paused`) ;
  - l'ouverture joue l'animation `rise` sur les cartes.

  La pilule montre l'heure (à la minute), l'orbe miniature et un emplacement pour la musique et les mails. Elle se mesure avec un `ResizeObserver` et appelle `pillResize`. Au survol prolongé (`hover_delay_ms`), elle ouvre le panneau si `hover_open` est activé.
- [ ] **Step 4 : Lancer** `pnpm test`. Attendu : PASS.
- [ ] **Step 5 : Vérifier à la main** avec `pnpm tauri dev` : l'aspect est conforme à `docs/design/maquette-panneau.png` pour la structure, l'en-tête, la frise et la barre. Faire une capture et la montrer à l'utilisateur.
- [ ] **Step 6 : Commit** : `feat(panneau): construit la coquille du panneau et de la pilule`

---

### Task 9 : Zone de notification, démarrage automatique, instance unique, notifications

**Files :**
- Create : `src-tauri/src/tray.rs`
- Modify : `src-tauri/src/lib.rs`
- Test : vérification manuelle (ces plugins n'ont pas de logique propre à tester)

**Interfaces :**
- Consumes : `Settings.autostart` (tâche 6), `toggle_panel` (tâche 3).
- Produces :
  - un menu dans la zone de notification avec « Ouvrir Oniris », « Paramètres » (ouvre le panneau, la section viendra au plan 7), « Mettre la voix en pause » (élément coché, sans effet avant le plan 6) et « Quitter » ;
  - `pub fn notify(app: &AppHandle, title: &str, body: &str)`, qui servira aux rappels et aux sondes.

- [ ] **Step 1 : Implémenter**.
  - Un clic gauche sur l'icône bascule le panneau.
  - L'instance unique : lancer une deuxième fois Oniris ouvre le panneau de l'instance déjà en cours.
  - Le démarrage automatique est synchronisé avec `Settings.autostart` à chaque enregistrement.
- [ ] **Step 2 : Vérifier à la main** :
  - un double lancement n'ouvre qu'une seule instance ;
  - l'icône et le menu fonctionnent ;
  - `notify` affiche une notification Windows avec le titre « Oniris ».
- [ ] **Step 3 : Commit** : `feat(systeme): ajoute la zone de notification, le démarrage auto et l'instance unique`

---

### Task 10 : Banc de mesure des performances et décision sur le framework

**Files :**
- Create : `src-tauri/src/perf.rs`, `tools/perf/measure.mjs`, `tools/perf/README.md`
- Modify : `src-tauri/src/commands.rs`, `src/panel/main.tsx`

**Interfaces :**
- Consumes : `open_panel`, `close_panel` et l'événement `oniris://panel-state` avec `at_ms` (tâche 3).
- Produces :
  - commande `perf_frame(at_ms: u64)` : le panneau l'appelle avec `Date.now()` au **deuxième** `requestAnimationFrame` qui suit l'événement d'ouverture ;
  - commande `perf_open_cycles(n: u32) -> PerfStats`, disponible **seulement en build debug ou avec la feature `perf`** ;
  - `pub struct PerfStats { pub samples: u32, pub p50_ms: f64, pub p95_ms: f64, pub max_ms: f64 }` ;
  - `pub fn percentile(sorted: &[f64], p: f64) -> f64`.

- [ ] **Step 1 : Écrire le test qui échoue** :

```rust
#[test] fn percentiles() {
    let v = [1.0, 2.0, 3.0, 4.0, 100.0];
    assert_eq!(percentile(&v, 0.5), 3.0);
    assert_eq!(percentile(&v, 1.0), 100.0);
}
```

- [ ] **Step 2 : Lancer** `cargo test -p oniris perf`. Attendu : FAIL. Implémenter `percentile`, puis relancer. Attendu : PASS.
- [ ] **Step 3 : Implémenter `perf_open_cycles`**. La commande enchaîne `n` cycles ouvrir, attendre `perf_frame`, fermer, attendre 300 ms, avec un délai maximal de 2 s par cycle. Elle calcule les écarts `frame_at_ms − open_at_ms`.
- [ ] **Step 4 : Écrire `tools/perf/measure.mjs`**. Il se connecte au port CDP `9333` (lancer le dev avec `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 pnpm tauri dev`), puis :
  1. appelle `perf_open_cycles(30)` ;
  2. une fois le panneau fermé et l'app au repos depuis 60 s, additionne via PowerShell la mémoire de travail privée d'`oniris.exe` et de tous les processus `msedgewebview2.exe` descendants (arbre des parents) ;
  3. mesure le temps CPU cumulé de ces processus sur 60 s, divisé par 60 s et par le nombre de cœurs ;
  4. écrit `tools/perf/results-<date>.json` et affiche un tableau comparé aux budgets (ouverture p95 < 100 ms, CPU < 0,5 %, RAM < 200 Mo).
- [ ] **Step 5 : Mesurer en dev** avec `node tools/perf/measure.mjs`. Attendu : un tableau affiché et un fichier JSON écrit.
- [ ] **Step 6 : Mesurer en release, avec l'accord de l'utilisateur**. Un build release est long : demander avant de lancer `cargo build --release -p oniris --features perf`, puis lancer l'exécutable avec le port CDP et relancer la mesure.
- [ ] **Step 7 : Décider du framework** (spec §8, §9).
  - Si l'ouverture p95 tient sous 100 ms et que la RAM reste sous 200 Mo, **React est confirmé** et on le note dans `tools/perf/README.md`.
  - Sinon, mesurer la part de React en comparant avec une page `panel.html` sans React qui contient le même DOM statique. Si l'écart dépasse 20 ms d'ouverture ou 30 Mo de RAM, **s'arrêter et présenter les chiffres à l'utilisateur** avec la proposition de passer à SolidJS, avant d'attaquer le plan 2.
  - Pistes à mesurer côté WebView2 : `--js-flags=--max-old-space-size`, `MemoryUsageTargetLevel = Low` quand le panneau est fermé (ICoreWebView2_19), et flou limité aux surfaces visibles.
- [ ] **Step 8 : Commit** : `perf(socle): mesure l'ouverture, la RAM et le CPU`

---

## Fin du plan 1 : critères

- `cargo test --workspace` et `pnpm test` passent ; `cargo clippy --workspace -- -D warnings` est propre ; `pnpm build` (TypeScript + Vite) réussit.
- Le comportement correspond à la spec §4.1 à §4.4, hors voix et réglages avancés : Alt+Espace (appui court / maintien), Échap, clic sur le voile, masquage en plein écran, zone de notification.
- Les mesures de la tâche 10 sont consignées, et la décision sur le framework est prise ou soumise à l'utilisateur.
- Mettre à jour la feuille de route (plan 1 coché), puis lancer `/graphify` sur le projet pour créer le graphe initial.
