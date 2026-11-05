# Oniris — Plan 4 : Comptes en ligne

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Brancher les services en ligne de l'utilisateur, sans coût mensuel et sans jamais agir dans son dos :
- **plusieurs comptes Gmail** : mails unifiés, lecture isolée, archivage, réponse confirmée ;
- l'**agenda Google** de ces comptes, en lecture seule ;
- **Spotify Premium** : ce qui joue, par Windows (SMTC), et le pilotage, par la Web API ;
- la **surveillance des sites**, plus la charge du VPS lue via le serveur MCP de Zenytt ;
- un **guide pas à pas** pour enregistrer ses propres applications Google et Spotify.

**Architecture :** Même découpage que les plans 2 et 3 :
- le cœur va dans `crates/core/src/<module>/` : clients HTTP sur une URL de base injectable, analyseurs purs, tables SQLite ;
- la colle Tauri va dans `src-tauri/src/modules/<module>.rs` : boucles de relève, SMTC, MCP ;
- l'interface va dans `src/modules/<module>/`.

Les jetons OAuth vivent dans le **gestionnaire d'identifiants Windows**, jamais en base ni dans un fichier. Chaque appel HTTP du cœur prend une `base_url` pour être testé contre un faux serveur local.

**Tech Stack :** Celle des plans 1 à 3, plus :
- `windows`, avec les features `Win32_Security_Credentials`, `Media_Control`, `Foundation` et `Storage_Streams` : coffre et SMTC ;
- `ammonia` : nettoyage du HTML des mails ;
- `rmcp`, avec les features client et child-process, la même bibliothèque que Zenytt : client MCP ;
- `wiremock` (dev) : faux serveurs HTTP sur `127.0.0.1`.

**Spec :** [`docs/superpowers/specs/2026-11-03-oniris-design.md`](../specs/2026-11-03-oniris-design.md), §6.2 (agendas externes), §6.3, §6.4, §6.8, §7.4, §7.5, §9, §10 et §11.

**Plans précédents et journaux :** [plan 3](2026-11-05-oniris-03-recherche-projets.md) et [journal 3](2026-11-05-oniris-03-recherche-projets-journal.md). Le journal liste les reports.

## Décisions prises avant ce plan (2026-11-05)

- **Pas de Microsoft Graph.** L'utilisateur n'a aucune adresse Outlook : toutes ses adresses sont des Gmail, sauf une adresse Orange.
  - L'adresse Orange est **transférée vers Gmail** côté Orange. Le guide l'explique, et Oniris n'a pas de code IMAP.
  - Un compte Microsoft personnel ne peut plus enregistrer d'application sans annuaire Azure (vérification par carte bancaire), ce qui confirme ce choix.
  - La tâche 12 met à jour la spec (§3, §6.2, §6.3, §10).
- **Spotify, mode développement de février 2026.**
  - Le propriétaire de l'application doit être Premium, ce que l'utilisateur est.
  - Cinq utilisateurs au plus, ce qui suffit.
  - La recherche est limitée à `limit=10`.
  - Les lecteurs (`/me/player/*`), `/me/playlists`, `/playlists/{id}/items` et `/search` restent disponibles.
  - Ne **pas** utiliser `/playlists/{id}/tracks`, les lots (`GET /tracks?ids=`), `/browse/*` ni `/artists/{id}/top-tracks`.
- **Google.** L'application de l'utilisateur est publiée « En production » sans vérification (spec §6.3).
  - Scopes : `openid`, `email`, `https://www.googleapis.com/auth/gmail.modify` et `https://www.googleapis.com/auth/calendar.readonly`.
  - `gmail.modify` couvre la lecture, l'archivage, la corbeille et l'envoi, mais pas la suppression définitive.

## Global Constraints

- **Héritées des plans précédents.** Les contraintes globales des plans 2 et 3 restent valables :
  - Windows 11 et budgets du §9 ;
  - une seule liste `CORE_MIGRATIONS` ;
  - nouvelles commandes déclarées dans `build.rs`, dans `capabilities/panel.json` et dans `generate_handler!` ;
  - les commandes qui touchent la base sont `#[tauri::command(async)]` ;
  - le chemin d'ouverture du panneau ne verrouille jamais la base ;
  - journal sans contenu utilisateur : adresses, sujets, titres, URL de sites et noms de playlists sont du contenu utilisateur ;
  - lancements sans console ;
  - commits signés, sans co-signature.
- **Migrations de ce plan :**
  - 13 : `accounts` ;
  - 14 : `mails` et `mail_sync` ;
  - 15 : `external_events` ;
  - 16 : `probes`.
- **Nouveaux `Topic`**, côté Rust et côté TypeScript : `Accounts`, `Mail`, `Music` et `Sites`. Les événements Google passent par `Agenda`.
- **Nouveaux réglages**, chacun avec défaut, validation, miroir TypeScript et tests :
  - `google_client_id: String` (vide = non configuré) ;
  - `spotify_client_id: String` ;
  - `mail_poll_minutes: u8`, entre 2 et 5, défaut 3 ;
  - `sites: Vec<Site>`, avec `Site { name: String /* 40 */, url: String /* http(s), 300 */, slow_ms: u32 /* 300..10000, défaut 1500 */ }`, 30 au plus ;
  - `zenytt_path: String` (vide = détection automatique).
- **Secrets :**
  - le secret client Google (type « application de bureau », non confidentiel mais traité comme un secret) et chaque jeton d'actualisation sont dans le gestionnaire d'identifiants Windows ;
  - noms de cible : `Oniris/google-client-secret` et `Oniris/account/<id>` ;
  - les jetons d'accès restent **en mémoire seulement** ;
  - rien de tout ça n'est jamais journalisé, renvoyé à l'interface ni écrit en base.
- **OAuth :**
  - code d'autorisation avec PKCE (S256) et paramètre `state` aléatoire vérifié ;
  - redirection en boucle locale **fixe** : `http://127.0.0.1:47829/callback`, écoute sur `127.0.0.1` seulement, une seule requête acceptée, 5 minutes au plus ;
  - le navigateur est ouvert par `open_url`, qui n'accepte que `https` ;
  - si le port est pris, message clair, sans autre port.
- **Réseau :**
  - client `reqwest` partagé, délai de 15 s ;
  - relèves espacées et incrémentales ;
  - **aucune relève en jeu plein écran** pour Spotify et Zenytt (`is_fullscreen`).
  - Les mails et les sondes continuent en plein écran : ce sont des requêtes légères et rares.
- **Outils et permissions** (spec §7.5) :
  - **Free** :
    - `mail.list`, `mail.read`, `mail.mark_read` (undo), `mail.archive` (undo), `mail.open_web` ;
    - `music.*` (lecture, playlist, recherche, volume, file, appareil) ;
    - `sites.status`, `vps.status`, `calendar.list` ;
  - **Confirm** :
    - `mail.delete` (corbeille Gmail, undo = sortie de corbeille) ;
    - `mail.send` (Risk High, sans undo).

    La carte de confirmation de `mail.send` montre le compte, le destinataire, le sujet et le texte exact.
- **Contenu des mails = non fiable.**
  - Le corps n'est jamais stocké en base : il est relu à la demande, avec un cache mémoire de 20 mails.
  - Le HTML passe par `ammonia`, puis s'affiche dans une `iframe` `sandbox` sans `allow-scripts`, avec une CSP qui bloque toute ressource distante.
  - Le bouton « Afficher les images » ne relâche que `img-src https:`.
  - Les liens passent par `safe_url` et ne s'ouvrent que sur clic de l'utilisateur.
- **R8 pendant l'exécution :**
  - pas de `tauri dev` ;
  - aucune connexion à un vrai compte, aucun vrai OAuth, aucun vrai mail ;
  - aucune requête vers Internet ;
  - les faux serveurs `wiremock` sur `127.0.0.1` sont permis ;
  - ne pas lancer Zenytt ni Spotify : le client MCP se teste contre un faux serveur MCP lancé par le test (un petit binaire de test ou un script en écho) ;
  - le test du vrai gestionnaire d'identifiants est `#[ignore]`.
  - Lire le code source de `Projets/zenytt` est permis ; le modifier ne l'est pas.

## Review Focus

1. **Jeton d'actualisation révoqué ou expiré** (`invalid_grant`). Le compte passe à « À reconnecter », sans boucle de relève ni rafale d'erreurs ; les autres comptes continuent. Tests dans les tâches 1 et 3.
2. **PC hors ligne.** Les cartes affichent le dernier état connu avec « hors ligne ». Les relèves reculent avec un délai croissant. Si **tous** les sites échouent en même temps, c'est le PC qui est hors ligne : aucune notification « site en panne ». Tests dans les tâches 3 et 10.
3. **Mail HTML hostile.**
   - `<script>`, `on*=`, pixels espions distants, CSS `url()`, liens `javascript:` et formulaires : rien ne s'exécute ni ne se charge avant le clic.
   - Mail de 5 Mo : tronqué à 1 Mo avec la mention « Ouvrir dans Gmail ».

   Tests dans les tâches 4 et 5.
4. **Spotify fermé ou sans appareil actif** (`404 NO_ACTIVE_DEVICE`). Transfert vers l'appareil « Computer » s'il existe, sinon message « Ouvre Spotify ». Sans session SMTC, la carte affiche « Rien en lecture ». Tests dans les tâches 7 et 8.
5. **Trou d'historique Gmail** (`historyId` trop ancien, réponse 404). Resynchronisation complète sans doublon, et un mail supprimé ailleurs disparaît du cache. Tests dans la tâche 3.

---

## Arborescence ajoutée par ce plan

```
crates/core/src/
  secrets.rs                                  trait SecretStore (+ mémoire pour les tests)
  oauth/{mod.rs,pkce.rs,providers.rs}         PKCE, URL d'autorisation, échange et actualisation
  accounts/{mod.rs,tools.rs}                  comptes (migration 13)
  mail/{mod.rs,gmail.rs,mime.rs,html.rs,sync.rs,tools.rs}   mails (migration 14)
  calendar/{mod.rs,google.rs}                 agendas externes (migration 15)
  music/{mod.rs,spotify.rs,tools.rs}          Web API Spotify
  sites/{mod.rs,tools.rs}                     sondes (migration 16)
src-tauri/src/
  secrets_win.rs                              CredWriteW / CredReadW / CredDeleteW
  oauth_loopback.rs                           écoute 127.0.0.1:47829
  modules/{accounts.rs,mail.rs,calendar.rs,smtc.rs,music.rs,sites.rs,zenytt.rs}
src/
  modules/accounts/{api.ts,AccountsSheet.tsx,guide.ts,accounts.css}
  modules/mail/{api.ts,MailCard.tsx,MailReader.tsx,Composer.tsx,mail.css}
  modules/music/{api.ts,MusicCard.tsx,music.css}
  modules/sites/{api.ts,SitesSection.tsx,sites.css}
docs/verifications/plan-04-comptes-en-ligne.md
```

---

### Task 1 : Coffre, OAuth et comptes

**Files :**
- Create : `crates/core/src/secrets.rs`, `crates/core/src/oauth/{mod.rs,pkce.rs,providers.rs}`, `crates/core/src/accounts/mod.rs`, `src-tauri/src/secrets_win.rs`, `src-tauri/src/oauth_loopback.rs`.
- Modify : `db.rs` (migration 13), `settings.rs` (+ TypeScript : `google_client_id`, `spotify_client_id`), `events.rs` (+ TypeScript : `Accounts`), `lib.rs` et `Cargo.toml`.

**Interfaces :**
- `pub trait SecretStore: Send + Sync { fn get(&self, key: &str) -> Option<String>; fn set(&self, key: &str, value: &str) -> Result<(), String>; fn delete(&self, key: &str); }`.
  - `MemorySecrets` sert aux tests.
  - `WinCredentials`, dans src-tauri, utilise un `CRED_TYPE_GENERIC`, une persistance `CRED_PERSIST_LOCAL_MACHINE` et une cible `Oniris/<key>`.
- `pub enum Provider { Google, Spotify }` et `pub struct ProviderConfig { auth_url, token_url, scopes: &'static [&'static str], client_id: String, client_secret: Option<String> }`.
  - `providers::google(id, secret)` : `access_type=offline` et `prompt=consent` (pour obtenir le jeton d'actualisation).
  - `providers::spotify(id)` : sans secret, PKCE seul. Scopes : `user-read-playback-state`, `user-modify-playback-state`, `user-read-currently-playing`, `playlist-read-private` et `playlist-read-collaborative`.
- `pkce::new() -> (verifier: String /* 64 caractères non réservés */, challenge: String /* base64url(sha256), sans remplissage */)`.
- `oauth::auth_url(cfg, redirect, state, challenge) -> String`.
- `oauth::exchange(http, token_url_base, cfg, code, verifier, redirect) -> Result<Tokens, OAuthError>`.
- `oauth::refresh(http, …, refresh_token) -> Result<Tokens, OAuthError>`.
  - `Tokens { access: String, refresh: Option<String>, expires_at_ms: i64 }`.
  - `OAuthError::{Revoked /* invalid_grant */, Network, Other(String)}`.
- Migration 13 : `accounts(id INTEGER PRIMARY KEY, provider TEXT NOT NULL, label TEXT NOT NULL /* adresse ou nom affiché */, color TEXT NOT NULL, status TEXT NOT NULL /* ok | reauth */, created_ms INTEGER NOT NULL) STRICT`, avec un index unique sur `(provider, label)`.
  - Couleurs attribuées dans l'ordre, dans une palette de 8 couleurs tirée de `tokens.css`.
- `accounts::{add, list, set_status, remove}`. Supprimer un compte efface aussi son secret ainsi que ses mails et événements, en cascade applicative.
- `pub struct TokenCache` en mémoire (`account_id -> Tokens`), avec `access_token(account_id) -> Result<String, OAuthError>`.
  - Elle actualise si l'expiration tombe dans moins de 60 s.
  - En cas de `Revoked`, elle passe le compte à `reauth` et émet `Topic::Accounts`.
  - Une seule actualisation à la fois par compte.
- `oauth_loopback::wait_code(state: &str, timeout) -> Result<String, String>`.
  - Elle écoute sur `127.0.0.1:47829`, accepte **une** requête `GET /callback?code=…&state=…` et vérifie `state`.
  - Elle répond par une petite page HTML en français (« Compte connecté, tu peux fermer cet onglet »).
  - Toute autre requête reçoit un 404 et l'écoute continue.

- [ ] **Step 1 : Tests** (wiremock pour le point de jeton).
  - Le `challenge` du vecteur de test RFC 7636 est exact.
  - `auth_url` contient `code_challenge_method=S256`, le `state`, les scopes séparés par un espace et la redirection exacte.
  - L'échange réussit et donne `expires_at_ms = now + expires_in*1000`.
  - Une réponse `400 {"error":"invalid_grant"}` donne `Revoked`.
  - `TokenCache` : deux appels concurrents ne font qu'une seule actualisation, et un jeton révoqué passe le compte à `reauth`.
  - L'écoute en boucle locale, liée sur un port de test : un `state` faux est refusé, le bon code est renvoyé, une seule requête utile est acceptée.
  - `MemorySecrets` : aller-retour. Un test `#[ignore]` fait l'aller-retour `WinCredentials` sur une cible `Oniris/test-<aléatoire>`, puis la supprime.
  - Supprimer un compte supprime son secret.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(comptes): ajoute le coffre Windows, OAuth PKCE et les comptes`

---

### Task 2 : Guide de configuration et feuille « Comptes »

**Files :**
- Create : `src-tauri/src/modules/accounts.rs`, `crates/core/src/accounts/tools.rs`, `src/modules/accounts/{api.ts,AccountsSheet.tsx,guide.ts,accounts.css}`, `accounts.test.tsx`.
- Modify : `state.rs` (secrets, `TokenCache`), `Header.tsx` (bouton « Comptes »), `Panel.tsx`, `build.rs`, `capabilities/panel.json` et `lib.rs`.

**Interfaces :**
- Commandes :
  - `accounts_list() -> Vec<AccountView { id, provider, label, color, status }>` ;
  - `accounts_set_google_app(client_id, client_secret)` : l'identifiant va dans les réglages, le secret dans le coffre ;
  - `accounts_set_spotify_app(client_id)` ;
  - `accounts_connect(provider) -> Result<AccountView, String>` ;
  - `accounts_remove(id)` ;
  - `accounts_set_color(id, color)`.
- Déroulé de `accounts_connect` :
  1. Générer PKCE et `state`.
  2. Ouvrir l'URL d'autorisation via `open_url`.
  3. Attendre `wait_code`.
  4. Échanger le code.
  5. Lire l'identité :
     - Google : `GET https://openidconnect.googleapis.com/v1/userinfo`, champ `email` ;
     - Spotify : `GET /v1/me`, champ `display_name`.
  6. Si le compte existe déjà, le passer à `ok` et remplacer son jeton d'actualisation. Sinon, l'ajouter.
  7. Ranger le jeton d'actualisation dans le coffre.
  8. Émettre `Topic::Accounts`.
- Outil `accounts.list` (Free, lecture).
- `AccountsSheet` : une feuille par-dessus le panneau, ouverte depuis l'en-tête. Elle s'ouvre seule au premier lancement s'il n'y a aucun compte ni identifiant client (`kv` `accounts_intro_seen`). Elle contient :
  - **Comptes connectés** : pastille de couleur (sélecteur parmi les 8), adresse, état. « À reconnecter » apparaît en orange avec un bouton, et un bouton supprime le compte.
  - **Ajouter un compte Gmail** et **Connecter Spotify**. Ces boutons sont désactivés tant que l'application correspondante n'est pas configurée, avec un lien « Configurer d'abord ».
  - **Guides pas à pas** (`guide.ts` : des étapes numérotées, des boutons « Ouvrir la console », des valeurs à copier avec un bouton Copier) :
    - **Google** :
      1. créer un projet sur console.cloud.google.com ;
      2. activer Gmail API et Google Calendar API ;
      3. écran de consentement « Externe », avec les 4 scopes ;
      4. **« Publier l'application »** (statut En production) ;
      5. créer un identifiant « Application de bureau » ;
      6. coller l'identifiant et le secret.

      La 1ʳᵉ connexion affiche « Google n'a pas validé cette application » : passer par « Paramètres avancés », puis « Accéder à… ».
    - **Spotify** :
      1. ouvrir developer.spotify.com/dashboard et créer une app ;
      2. URI de redirection exacte `http://127.0.0.1:47829/callback` ;
      3. cocher « Web API » ;
      4. coller le Client ID.

      Le compte propriétaire doit être Premium.
    - **Orange vers Gmail** : dans le webmail Orange, Préférences, puis Transfert vers l'adresse Gmail. Dans Gmail, filtrer par l'adresse Orange pour l'étiqueter. Aucun code dans Oniris.

- [ ] **Step 1 : Tests.**
  - Côté TypeScript :
    - feuille vide avec les guides visibles ;
    - bouton Gmail désactivé sans identifiant client ;
    - les valeurs à copier contiennent exactement l'URI de redirection ;
    - compte `reauth` avec un bouton « Reconnecter » qui appelle `connect` ;
    - changement de couleur.
  - Côté Rust : `connect` avec un faux échange, puis un compte existant passe à `ok` sans doublon.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(comptes): ajoute la feuille Comptes et les guides de configuration`

---

### Task 3 : Gmail, client, synchronisation et cache

**Files :**
- Create : `crates/core/src/mail/{mod.rs,gmail.rs,sync.rs}`, `crates/core/tests/fixtures/gmail/*.json`, `src-tauri/src/modules/mail.rs` (boucle de relève).
- Modify : `db.rs` (migration 14), `settings.rs` (+ TypeScript : `mail_poll_minutes`), `events.rs` (+ TypeScript : `Mail`) et `state.rs`.

**Interfaces :**
- Migration 14 :
  - `mails(account_id INTEGER NOT NULL, id TEXT NOT NULL, thread_id TEXT NOT NULL, from_name TEXT NOT NULL, from_addr TEXT NOT NULL, subject TEXT NOT NULL, snippet TEXT NOT NULL, date_ms INTEGER NOT NULL, unread INTEGER NOT NULL, labels TEXT NOT NULL /* JSON */, bucket TEXT NOT NULL /* todo | read | filtered */, bucket_src TEXT NOT NULL /* heuristic | iris */, summary TEXT, tag TEXT, PRIMARY KEY(account_id, id)) STRICT`, avec un index sur `date_ms`.
  - `mail_sync(account_id INTEGER PRIMARY KEY, history_id TEXT, last_ok_ms INTEGER, backoff_until_ms INTEGER) STRICT`.
- `gmail::Client { http, base: String /* https://gmail.googleapis.com */ }`, avec :
  - `list_inbox(token, max: 100) -> Vec<String /*id*/>` ;
  - `get_metadata(token, id) -> MetaMsg`, avec `format=metadata` et les en-têtes `From`, `Subject`, `Date` et `List-Unsubscribe` ;
  - `history(token, start) -> Result<HistoryDelta { added, removed, label_changed, new_history_id }, GmailError>`, où `GmailError::HistoryGone` correspond à une réponse 404 ;
  - `get_full(token, id) -> FullMsg { html: Option<String>, text: Option<String>, attachments: Vec<AttachmentMeta> }`, au format `full` (décodage base64url et parcours des parties MIME) ;
  - `modify(token, id, add: &[&str], remove: &[&str])`, `trash(id)`, `untrash(id)` et `send(token, raw_b64url, thread_id)`.
- `sync::run(c, client, tokens, account_id, now) -> Result<SyncReport { added, removed, changed }, SyncError>` :
  - sans `history_id`, synchronisation complète : les 100 derniers mails de la boîte de réception ;
  - sinon, synchronisation par l'historique ;
  - sur `HistoryGone`, synchronisation complète puis remplacement du cache du compte, sans doublon ;
  - un mail qui sort de la boîte de réception (archivé, supprimé) est retiré du cache ;
  - le cache est borné à 500 mails par compte.
- `fn heuristic_bucket(labels, has_list_unsubscribe) -> Bucket` :
  - `filtered` pour `CATEGORY_PROMOTIONS`, `CATEGORY_SOCIAL`, `CATEGORY_UPDATES` ou `CATEGORY_FORUMS`, ou si `List-Unsubscribe` est présent ;
  - sinon `todo` si `IMPORTANT` est présent ;
  - sinon `read`.

  Toujours avec `bucket_src = 'heuristic'`. Le plan 5 (IRIS) remplacera ce tri sans toucher aux lignes `iris`.
- Boucle de relève, côté Tauri :
  - une relève par compte `ok` toutes les `mail_poll_minutes` ;
  - une relève tout de suite à l'ouverture du panneau si la dernière date de plus de 60 s ;
  - délai croissant après une erreur réseau : 1, 2, 4, 8 puis 15 minutes au plus ;
  - `Revoked` passe le compte en `reauth` et arrête sa relève.

  Elle émet `Topic::Mail` seulement s'il y a eu un changement.

- [ ] **Step 1 : Tests** (fixtures JSON et wiremock).
  - Synchronisation complète, puis synchronisation par l'historique : ajout, retrait et passage à lu.
  - Une réponse 404 sur l'historique provoque une resynchronisation complète sans doublon.
  - Un mail archivé ailleurs disparaît.
  - Analyse de `From: "Dupont, Jean" <j@x.fr>` et d'un sujet encodé en RFC 2047 (`=?UTF-8?B?…?=`).
  - Table de `heuristic_bucket`.
  - Délai croissant : deux erreurs réseau donnent 2 minutes.
  - `invalid_grant` passe le compte en `reauth`, sans nouvel essai.
  - Borne de 500 mails.
  - Le journal ne contient ni adresse ni sujet : vérifié par un relevé de `tracing` en test.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(mails): synchronise Gmail en incrémental avec un cache local`

---

### Task 4 : Outils mails et rendu sûr

**Files :**
- Create : `crates/core/src/mail/{mime.rs,html.rs,tools.rs}`.
- Modify : `mail/mod.rs`, `src-tauri/src/modules/mail.rs` (commandes), `state.rs`, `build.rs`, `capabilities/panel.json`, `lib.rs` et `Cargo.toml` (`ammonia`).

**Interfaces :**
- `html::sanitize(html: &str) -> Sanitized { html: String, links: Vec<String> /* data-href dans l'ordre, sans doublon, 50 au plus */ }`, avec `ammonia` :
  - balises de texte et de mise en forme seulement ;
  - pas de `script`, `style` externe, `form`, `iframe`, `object` ni `on*` ;
  - attribut `style` en ligne conservé, mais `url(` retiré ;
  - images gardées avec `src` http(s) ou `cid:` ;
  - liens réécrits en `data-href` (aucun `href` actif), pour être ouverts par l'interface via `mail.open_link` ;
  - entrée tronquée à 1 Mo.
- `html::text_to_html(text)` échappe le texte et rend les URL cliquables, sous la même forme `data-href`.
- `mime::reply(from, to, subject, body_text, in_reply_to, references) -> String` :
  - en-têtes en RFC 2047 si besoin ;
  - corps `text/plain; charset=utf-8` en quoted-printable ou en base64 ;
  - `Re: ` ajouté une seule fois ;
  - le résultat est encodé en base64url.
- Outils :
  - `mail.list {bucket?, account?, limit≤50}` (Free) renvoie des lignes du cache ;
  - `mail.read {account, id}` (Free) renvoie `{html, text, attachments, links}` et marque le mail comme lu, avec undo ;
  - `mail.mark_read {account, id, read: bool}` (Free, undo) ;
  - `mail.archive {account, id}` (Free, undo) : retirer `INBOX`, et l'undo le remet ;
  - `mail.delete {account, id}` (Confirm, undo : `untrash`). La carte de confirmation montre l'expéditeur et le sujet ;
  - `mail.send {account, to, subject, body, reply_to_id?}` (Confirm, `Risk::High`, sans undo, `sensitive_output`). La carte montre le compte, le destinataire, le sujet et le texte intégral ;
  - `mail.open_web {account, id}` (Free) ouvre `https://mail.google.com/mail/u/<adresse>/#all/<thread_id>` ;
  - `mail.open_link {url}` (Free si l'appel vient de l'utilisateur ; l'URL passe par `safe_url`).
- Le corps lu est gardé dans un cache mémoire LRU de 20 mails et n'est jamais écrit en base.

- [ ] **Step 1 : Tests.**
  - `sanitize` retire :
    - `<script>` ;
    - `onerror=` ;
    - `<img src=x onerror>` ;
    - `style="background:url(https://track)"` ;
    - `<a href="javascript:…">` (devient `data-href` puis est refusé par `safe_url`) ;
    - `<form>` et `<meta http-equiv=refresh>`.
  - Un HTML de 5 Mo est tronqué.
  - `reply` :
    - un sujet accentué est encodé ;
    - pas de `Re: Re:` ;
    - `In-Reply-To` et `References` sont présents ;
    - pas d'injection d'en-tête : un `\r\n` dans le sujet ou le destinataire est refusé.
  - `mail.send` exige une confirmation et la carte contient le texte exact. Rejet : aucun appel HTTP (wiremock vérifie zéro requête).
  - `archive` puis undo : `modify` est appelé avec `INBOX` en retrait puis en ajout.
  - `delete` : confirmation, puis `trash` ; l'undo appelle `untrash`.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(mails): ajoute les outils mails et le rendu HTML isolé`

---

### Task 5 : Carte Mails, lecteur et réponse

**Files :**
- Create : `src/modules/mail/{api.ts,MailCard.tsx,MailReader.tsx,Composer.tsx,mail.css}` et `mail.test.tsx`.
- Modify : `Panel.tsx` et `CardGrid.tsx` (emplacement `mails`).

**Interfaces :**
- `MailCard` occupe l'emplacement `mails`, selon la maquette.
  - Trois onglets, chacun avec son compteur de non-lus : « À traiter », « À lire » et « Filtrés ».
  - Une ligne de mail montre :
    - l'avatar avec ses initiales et la pastille de couleur du compte ;
    - l'expéditeur, le sujet et, si elle existe, l'étiquette (`tag`) ;
    - le résumé, ou à défaut l'extrait (`snippet`) ;
    - l'heure relative ;
    - un point non-lu.
  - Au survol apparaissent « Archiver », « Lu » et « Supprimer ».
  - Sans compte, la carte affiche « Connecte un compte Gmail » et ouvre la feuille Comptes. Un compte à reconnecter montre un bandeau orange.
  - Hors ligne, la carte affiche la mention « hors ligne · il y a N min » avec le cache.
- `MailReader` s'ouvre en volet par-dessus la carte (ou en grand), sans fermer le panneau.
  - L'`iframe` `sandbox="allow-popups-to-escape-sandbox"` n'a **pas** de `allow-scripts` ni de `allow-same-origin`.
  - `srcdoc` porte la CSP `default-src 'none'; style-src 'unsafe-inline'; img-src data: cid:`.
  - « Afficher les images » ajoute `https:` à `img-src`, pour ce mail seulement.
  - Les liens ne sont pas cliquables dans l'`iframe` : sans `allow-same-origin`, le parent ne peut pas l'écouter. Ils sont donc listés **sous** le mail (« Liens du message »), à partir de `links` renvoyé par `mail.read`, et chacun s'ouvre par `mail.open_link`.
  - Boutons : Répondre, Archiver, Supprimer, « Ouvrir dans Gmail ».
  - « Répondre avec IRIS » est grisé avec l'infobulle « Bientôt (IRIS) ».
- `Composer` :
  - champs À (prérempli), Sujet (`Re: …`) et texte ;
  - « Envoyer » appelle `mail.send`, ce qui fait apparaître la carte de confirmation du `ConfirmQueue` ;
  - le brouillon est gardé en mémoire tant que le panneau reste ouvert.

- [ ] **Step 1 : Tests (Vitest).**
  - Les onglets filtrent, et les compteurs sont justes.
  - La pastille porte la couleur du compte.
  - Le survol puis « Archiver » appelle `archive`.
  - Le lecteur rend une `iframe` sans `allow-scripts`, et sa CSP ne contient pas `https:` avant le clic sur « Afficher les images ».
  - Les liens sont listés et s'ouvrent via `open_link`.
  - Composer : « Envoyer » appelle `send` avec les champs exacts.
  - Sans compte : l'appel à l'action est présent.
  - Hors ligne : la mention est affichée.
- [ ] **Step 2 :** RED, puis GREEN. Capture de l'interface sans navigateur, sur les fausses données. Commit : `feat(mails): ajoute la carte Mails, le lecteur et la réponse`

---

### Task 6 : Agenda Google en lecture seule

**Files :**
- Create : `crates/core/src/calendar/{mod.rs,google.rs}`, `crates/core/tests/fixtures/gcal/*.json` et `src-tauri/src/modules/calendar.rs`.
- Modify : `db.rs` (migration 15), `agenda` (fusion à la lecture), `src/modules/agenda/*` et `Timeline.tsx`.

**Interfaces :**
- Migration 15 : `external_events(account_id INTEGER NOT NULL, id TEXT NOT NULL, calendar_id TEXT NOT NULL, title TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, all_day INTEGER NOT NULL, location TEXT, PRIMARY KEY(account_id, calendar_id, id)) STRICT`, avec un index sur `start_ms`. S'y ajoute `kv` `gcal_sync/<account>/<calendar>`, qui porte le `syncToken`.
- `google::list_calendars(token)`. On ne garde que `selected=true`.
- `google::events(token, cal, sync_token | (time_min, time_max)) -> EventsPage { items, next_sync_token, gone: bool /* 410 */ }`.
  - La fenêtre va d'aujourd'hui −7 jours à +60 jours, avec `singleEvents=true`.
  - Les événements `cancelled` sont retirés.
- La relève suit le rythme des mails (même boucle, même délai croissant) et émet `Topic::Agenda`.
- `agenda_between` renvoie aussi les événements externes, sous la forme `Event { …, source: "local" | "google", color?: String, readonly: bool }`.
  - Les événements externes ne sont ni déplaçables ni supprimables.
  - Leur couleur est celle du compte.
- Outil `calendar.list {from_ms, to_ms}` (Free) : il renvoie les événements locaux et externes.

- [ ] **Step 1 : Tests.**
  - Événement « toute la journée » (`date`, pas `dateTime`) : il couvre la journée locale.
  - Fuseau horaire de l'événement converti correctement.
  - Une réponse 410 provoque une resynchronisation complète.
  - Un événement `cancelled` est retiré.
  - Fusion : tri par heure, `readonly` vrai pour les événements externes.
  - Côté TypeScript : un bloc externe affiché dans la frise, sans poignée de déplacement.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(agenda): affiche les agendas Google en lecture seule`

---

### Task 7 : En lecture, par Windows (SMTC)

**Files :**
- Create : `src-tauri/src/modules/smtc.rs` et `crates/core/src/music/mod.rs` (types et extraction des couleurs).
- Modify : `Cargo.toml` (features `windows`), `events.rs` (+ TypeScript : `Music`), `windows.rs` (accroches d'ouverture et de fermeture) et `lib.rs`.

**Interfaces :**
- `pub struct NowPlaying { title, artist, album, cover: Option<String /* data:image/png, 300 px */>, colors: Vec<String /* 3 couleurs dominantes en hexadécimal */>, playing: bool, position_ms, duration_ms, updated_ms, source: "spotify" | "other" }`.
- `music::dominant_colors(img: &RgbaImage, k: 3) -> Vec<[u8;3]>` : histogramme quantifié sur 4 bits par canal, les pixels trop sombres ou trop clairs sont ignorés, et les couleurs sont triées par saturation × poids. L'algorithme est déterministe.
- `smtc` :
  - Il s'abonne à `GlobalSystemMediaTransportControlsSessionManager`, **seulement panneau ouvert**, puis se désabonne à la fermeture.
  - Il préfère la session dont `SourceAppUserModelId` contient `Spotify`, sinon il prend la session courante.
  - Les événements `MediaPropertiesChanged`, `PlaybackInfoChanged` et `TimelinePropertiesChanged` mettent à jour un `NowPlaying` en mémoire et émettent `Topic::Music`, au plus 4 fois par seconde.
  - La pochette n'est relue que si le titre change.
- Commandes et outils (Free, par SMTC, sans réseau) :
  - `music.toggle` ;
  - `music.next` ;
  - `music.previous` ;
  - `music.seek {position_ms}` : `TryChangePlaybackPositionAsync`, avec repli sur la Web API (tâche 8) si la réponse est `false`.
- `music_now() -> Option<NowPlaying>`.

- [ ] **Step 1 : Tests.**
  - `dominant_colors` sur une image synthétique moitié rouge, moitié bleu : on retrouve rouge et bleu, et le blanc est ignoré.
  - Sélection de session : parmi `["Chrome", "Spotify.exe"]`, c'est Spotify qui est choisi. Ce test porte sur une fonction pure de sélection.
  - Limitation à 4 émissions par seconde.
  - Le vrai SMTC n'est **pas** testé. Il va dans la liste de vérification.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(musique): lit le titre en cours via Windows (SMTC)`

---

### Task 8 : Spotify Web API et outils musique

**Files :**
- Create : `crates/core/src/music/{spotify.rs,tools.rs}`, `crates/core/tests/fixtures/spotify/*.json` et `src-tauri/src/modules/music.rs`.
- Modify : `state.rs`, `build.rs`, `capabilities/panel.json` et `lib.rs`.

**Interfaces :**
- `spotify::Client { http, base: "https://api.spotify.com/v1" }` :
  - `my_playlists(token, limit 50) -> Vec<Playlist { id, name, image: Option<url>, owner_is_me }>` ;
  - `devices(token) -> Vec<Device { id, name, kind, active, volume }>` ;
  - `play(token, device?, PlayTarget::{Context(uri), Tracks(Vec<uri>)})` ;
  - `transfer(token, device_id, play: bool)` ;
  - `search(token, q, types: "track,artist,playlist", limit ≤ 10) -> SearchResult` ;
  - `volume(token, percent 0..=100)` ;
  - `queue(token, uri)` ;
  - `seek(token, ms)`.
- `SpotifyError::{NoActiveDevice /* 404 NO_ACTIVE_DEVICE */, NotPremium /* 403 PREMIUM_REQUIRED */, RateLimited(retry_after_s), Revoked, Other}`.
- `with_device(client, op)` : sur `NoActiveDevice`, transfert vers le premier appareil `kind == "Computer"` puis nouvel essai, une seule fois. Sans appareil, l'erreur conviviale est « Ouvre Spotify sur ce PC ».
- Outils, tous Free :
  - `music.play_playlist {id | name}` : recherche du nom dans `my_playlists`, insensible à la casse et aux accents, préfixe d'abord ;
  - `music.play_search {query, kind: track|artist|playlist}` : premier résultat. Pour un artiste, on joue son contexte d'artiste ;
  - `music.volume {percent | delta}` : `delta` part du volume de l'appareil actif ;
  - `music.queue {query}` ;
  - `music.devices` ;
  - `music.transfer {device_id}`.
- Playlists favorites : `kv` `music_favorites` (liste d'identifiants, 8 au plus). Si elle est vide, ce sont les 6 premières de `my_playlists`. Elles sont récupérées une fois à l'ouverture du panneau et mises en cache 10 minutes.
- Aucune requête Spotify en plein écran, ni panneau fermé, sauf demande explicite d'un outil.

- [ ] **Step 1 : Tests** (wiremock et fixtures).
  - `NoActiveDevice` déclenche un transfert vers « Computer » puis un nouvel essai, une seule fois.
  - Sans appareil : erreur « Ouvre Spotify ».
  - `PREMIUM_REQUIRED` donne `NotPremium` et un message clair.
  - 429 avec `Retry-After` : aucun nouvel essai avant le délai.
  - `play_playlist` sur « focus » trouve « Focus Deep Work ».
  - `search` envoie `limit=10` au plus.
  - `volume` avec un delta de −10 depuis 35 donne 25, et la valeur est bornée à 0..100.
  - Aucune route `/tracks?ids=` ni `/playlists/{id}/tracks` n'est appelée (wiremock vérifie les chemins).
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(musique): pilote Spotify via la Web API`

---

### Task 9 : Carte « En lecture »

**Files :**
- Create : `src/modules/music/{api.ts,MusicCard.tsx,music.css}` et `music.test.tsx`.
- Modify : `Panel.tsx`, `CardGrid.tsx` (emplacement `music`) et `Panel.css` (lumière ambiante).

**Interfaces :**
- `MusicCard` suit la maquette.
  - Pochette de 72 px, titre et artiste.
  - Forme d'onde animée en CSS (barres sur `transform: scaleY`), en pause quand la lecture s'arrête ou que `reduce_motion` est actif.
  - Barre de progression déplaçable (pointeur et clavier ←/→ de 5 s), qui appelle `music.seek`. La position avance localement entre deux événements, avec un seul `requestAnimationFrame`, seulement quand la carte est visible.
  - Boutons précédent, lecture/pause et suivant.
  - Vignettes des playlists favorites. Un clic lance `play_playlist`, et le menu « … » permet d'épingler ou de retirer.
  - Petit sélecteur d'appareil.
- Lumière ambiante : les `colors` de `NowPlaying` alimentent les variables CSS `--ambient-1/2/3` du panneau, avec une transition de 1,2 s sur l'opacité d'un calque. Il n'y a aucune animation de `background` en continu.
- États de la carte :
  - « Rien en lecture » ;
  - Spotify non connecté : le pilotage SMTC reste actif, avec l'invitation « Connecte Spotify pour les playlists » ;
  - erreur conviviale.

- [ ] **Step 1 : Tests (Vitest).**
  - Les boutons appellent `toggle`, `next` et `prev`.
  - Glisser sur la barre appelle `seek` avec la bonne position.
  - Un clic sur une vignette appelle `play_playlist(id)`.
  - L'onde est figée en pause.
  - Les variables `--ambient-*` sont posées depuis `colors`.
  - Les états « rien en lecture » et « non connecté » s'affichent.
- [ ] **Step 2 :** RED, puis GREEN. Capture sans navigateur. Commit : `feat(musique): ajoute la carte En lecture et la lumière ambiante`

---

### Task 10 : Surveillance des sites

**Files :**
- Create : `crates/core/src/sites/{mod.rs,tools.rs}`, `src-tauri/src/modules/sites.rs` et `src/modules/sites/{api.ts,SitesSection.tsx,sites.css}`.
- Modify : `db.rs` (migration 16), `settings.rs` (+ TypeScript : `sites`), `events.rs` (+ TypeScript : `Sites`), `FavoritesCard.tsx` (section VPS au-dessus, carte renommée « VPS et favoris ») et `confidentialite.md`.

**Interfaces :**
- Migration 16 : `probes(site TEXT NOT NULL /* url */, at_ms INTEGER NOT NULL, ms INTEGER, status INTEGER, ok INTEGER NOT NULL) STRICT`, avec un index sur `(site, at_ms)`. Les lignes de plus de 24 h sont purgées à chaque tour.
- `sites::probe(http, url, timeout 10 s) -> Probe { ms, status, ok }`.
  - Requête `GET` avec au plus 3 redirections, sans cookie, `User-Agent: Oniris-sonde/<version>`.
  - Le corps est lu sur 64 Ko au plus puis abandonné.
  - `ok` correspond à un code de 200 à 399.
- `sites::state(history: &[Probe], slow_ms) -> SiteState::{Up, Slow, Down, Unknown}` :
  - `Down` après 2 échecs consécutifs ;
  - `Slow` si les 3 dernières mesures dépassent `slow_ms` ;
  - le retour à `Up` est immédiat.
- `sites::transitions(prev, next, all_failed: bool) -> Vec<Notice>` :
  - une notification quand un site passe à `Down`, à `Slow` ou revient à `Up` après `Down` ;
  - **aucune** si `all_failed` (le PC est hors ligne). La carte affiche alors « Hors ligne ».
- Une boucle Tauri tourne toutes les 60 s, **même panneau fermé**, et sonde les sites en parallèle (4 à la fois). Elle émet `Topic::Sites` et appelle `tray::notify` pour les transitions.
- Outil `sites.status` (Free). Commandes `sites_list() -> Vec<SiteView { name, url, state, last_ms, spark: Vec<u32 /* 30 dernières mesures */> }>` et `sites_set(Vec<Site>)`.
- `SitesSection` :
  - titre « 6/6 en ligne » ;
  - pastilles (vert, orange, rouge) ;
  - temps de réponse ;
  - petite courbe en SVG ;
  - édition de la liste (ajouter, retirer) ;
  - la charge du VPS (tâche 11) en tête si elle est disponible.

- [ ] **Step 1 : Tests.**
  - `state` :
    - 1 échec donne encore `Up` ;
    - 2 échecs donnent `Down` ;
    - 3 mesures lentes donnent `Slow`.
  - `transitions` : `all_failed` ne produit aucune notification, et Down → Up produit une notification de rétablissement.
  - `probe` contre wiremock : 200, 503, redirection, délai dépassé.
  - Purge des mesures de plus de 24 h.
  - Validation des réglages : URL `http(s)` uniquement, 30 sites au plus.
  - Côté TypeScript : compteur « N/M en ligne », couleur des pastilles, ajout d'un site.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(sites): surveille les sites et notifie les pannes`

---

### Task 11 : Charge du VPS via Zenytt (MCP)

**Files :**
- Create : `src-tauri/src/modules/zenytt.rs` et `src-tauri/tests/fake_mcp.rs` (ou un binaire de test).
- Modify : `settings.rs` (+ TypeScript : `zenytt_path`), `SitesSection.tsx`, `Cargo.toml` (`rmcp`), `build.rs` et `capabilities/panel.json`.

**Interfaces :**
- Lire d'abord `Projets/zenytt/crates/mcp/src/lib.rs`, en lecture seule. Outils utilisés :
  - `list_servers` (sans argument), qui renvoie `[{name, host, id}]` ;
  - `server_status {server}`, qui renvoie `{metrics, memPercent, agentInstalled, activeAlerts}` ;
  - `list_sites {server}`, pour importer les domaines.

  Vérifier les noms exacts des champs de `metrics` (CPU, disques, charge) dans `zenytt_protocol`.
- `zenytt_exe() -> Option<PathBuf>` :
  - le réglage, s'il est rempli ;
  - sinon `%LOCALAPPDATA%\Programs\Zenytt\Zenytt.exe` ou `%LOCALAPPDATA%\Zenytt\Zenytt.exe` ;
  - sinon `None`, et la section VPS est masquée.
- `ZenyttClient` :
  - processus `Zenytt.exe --mcp` lancé avec `CREATE_NO_WINDOW`, en client `rmcp` sur stdio ;
  - démarré à la **première ouverture du panneau** qui en a besoin ;
  - arrêté 10 minutes après la fermeture du panneau ;
  - interroge `server_status` toutes les 30 s **panneau ouvert seulement**, jamais en plein écran ;
  - délai de 25 s par appel (la connexion SSH peut prendre 20 s) ;
  - toutes les erreurs deviennent un état « VPS indisponible » avec la raison courte que donne Zenytt.
- `vps_status() -> Option<VpsView { server, cpu_pct, mem_pct, disk_pct, load1, alerts: u32, at_ms }>`, et l'outil `vps.status` (Free).
- `sites_import_from_zenytt()` : il appelle `list_sites` sur chaque serveur et ajoute les domaines HTTPS absents à `sites`. Oniris ne gère **aucun** identifiant SSH.

- [ ] **Step 1 : Tests.** Un faux serveur MCP sur stdio, petit binaire de test écrit avec `rmcp` côté serveur, expose `list_servers`, `server_status` et `list_sites` avec des réponses figées.
  - Le client affiche les métriques.
  - Délai dépassé : état indisponible.
  - Processus mort : redémarrage au tour suivant.
  - Import des sites : sans doublon, HTTPS uniquement.
  - Arrêt après 10 minutes, testé avec une horloge injectée.
- [ ] **Step 2 :** RED, puis GREEN. Commit : `feat(vps): lit la charge du VPS via le serveur MCP de Zenytt`

---

### Task 12 : Intégration, confidentialité, spec et liste de vérification

**Files :**
- Create : `docs/verifications/plan-04-comptes-en-ligne.md`.
- Modify : `docs/legal/confidentialite.md`, `docs/superpowers/specs/2026-11-03-oniris-design.md` et `docs/superpowers/plans/2026-11-03-oniris-feuille-de-route.md`.

- [ ] **Step 1 :** Capture complète du panneau en 2560×1440 et en 1920×1080. Le panneau doit toujours tenir en 1440p, et la bande vide sous les cartes « Bientôt » doit avoir disparu.
- [ ] **Step 2 :** Ajouter à `confidentialite.md` :
  - **Google** : Gmail (métadonnées et extrait en cache local, corps jamais stockés) et Agenda (événements en cache) ;
  - **Spotify** : commandes et playlists ;
  - **SMTC** : local ;
  - **sondes HTTP** vers les sites listés ;
  - **Zenytt** : processus local, sans identifiant SSH géré par Oniris ;
  - le stockage des jetons dans le gestionnaire d'identifiants Windows ;
  - comment tout révoquer (myaccount.google.com/permissions, spotify.com/account/apps).
- [ ] **Step 3 :** Mettre à jour la spec :
  - Gmail seulement ;
  - Outlook abandonné (Orange par transfert) ;
  - restrictions Spotify de février 2026 ;
  - date de la décision.
- [ ] **Step 4 :** Rédiger la liste de vérification, au format des plans précédents. Points à couvrir :
  - configuration Google et Spotify en suivant le guide ;
  - connexion de deux comptes Gmail ;
  - écran « non validée » ;
  - relève et onglets ;
  - mail HTML avec images bloquées ;
  - lien ;
  - archivage puis annulation ;
  - suppression avec confirmation ;
  - **réponse envoyée à soi-même uniquement** ;
  - transfert Orange ;
  - agenda Google dans la frise ;
  - SMTC avec Spotify et la musique d'un navigateur ;
  - playlists ;
  - Spotify fermé ;
  - lumière ambiante ;
  - site en panne simulé (URL invalide) ;
  - PC hors ligne sans rafale de notifications ;
  - Zenytt détecté, charge du VPS, import des sites ;
  - révocation d'un accès puis bandeau « À reconnecter ».

  Marquer « ⚠️ à vérifier en priorité » : OAuth Google en production, réponse à soi-même, SMTC réel et Zenytt réel.
- [ ] **Step 5 :** Cocher le plan 4 dans la feuille de route. Commit : `docs(verifications): liste les contrôles manuels du plan 4`

---

## Fin du plan 4 : critères

- Ces commandes passent : `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm test` et `pnpm build`.
- Migrations 13 à 16 : versions contiguës, chacune testée.
- Aucun jeton ni secret en base, dans un fichier ou dans le journal : vérifié par un test qui relève `tracing` pendant un échange OAuth simulé.
- Aucun envoi de mail possible sans carte de confirmation : test du courtier.
- Panneau fermé : aucune requête Spotify ni Zenytt. Seules les relèves de mails et d'agenda et les sondes tournent.
- `confidentialite.md` et la spec sont à jour, et le graphe graphify est mis à jour (`graphify update .`).
