# Oniris — document de conception

*Date : 2026-11-03 · Statut : en attente de validation*

Oniris est une application de bureau pour Windows 11 : un tableau de bord personnel qui vit en haut de l'écran, avec à l'intérieur **IRIS**, une assistante à voix féminine à qui l'on parle ou écrit, dans l'esprit de JARVIS/FRIDAY (Iron Man). Une seule personne l'utilise : son auteur, développeur, qui jongle entre plusieurs projets, plusieurs boîtes mail et Spotify toute la journée.

Les passages marqués **[choix de conception]** sont des décisions prises par le concepteur, non discutées avec l'utilisateur : il peut les remettre en cause en relisant.

---

## 1. Intention et raison d'être

Aujourd'hui, l'utilisateur :

- passe par un dashboard web (son projet `Projets/Nexus_Dashboard`, Next.js auto-hébergé) surtout pour ses favoris et raccourcis web ; il ne lui convient plus ;
- lit chaque jour ses mails sur plusieurs adresses Gmail, boîte par boîte, et y perd du temps ;
- fait des alt-tab pour changer de musique dans Spotify ;
- cherche ses fichiers et projets à la main ;
- reprend ses projets de dev ainsi : ouvrir le projet dans VS Code, relire le plan de développement, demander à Claude (extension VS Code) de « continuer ce sur quoi il travaillait », soit en mode normal, soit en mode orchestrateur multi-agents pour les projets poussés.

**Le fil conducteur : plus d'alt-tab, plus de recherche, plus de reconstitution de contexte.** Tout ce dont il a besoin au quotidien est à un raccourci de distance, et IRIS peut le faire à sa place, à l'écrit ou à la voix.

**Ce qui fait l'« âme » de l'app** (validé sur maquette, cf. §5) : l'assistante est au centre (sphère vivante + briefing qui raconte la journée et propose des actions), l'interface réagit à ce qui se passe (lumière ambiante tirée de la pochette en cours, frise du jour avec un repère « maintenant » qui avance), et tout est actionnable en un clic.

**Ce à quoi ressemble la réussite :** il ouvre le panneau le matin, sait en dix secondes ce qui l'attend, et la plupart des petites actions de la journée (musique, mails, reprise de projet, rappels, recherche) passent par Oniris sans quitter ce qu'il fait. L'app reste invisible en ressources quand il joue ou compile.

## 2. Objectifs, non-objectifs, anti-objectifs

### Objectifs

- Une **pilule** permanente en haut de l'écran qui se déroule en **panneau** tableau de bord (§4).
- Les modules décrits au §6 : briefing, frise du jour, agenda, rappels, mails unifiés triés par l'IA, Spotify, favoris, recherche universelle, projets avec reprise Claude, état du VPS et des sites, état du PC, météo, historique du presse-papiers, notes rapides, minuteur.
- **IRIS** (§7) : texte et voix, capable d'agir sur tous les modules, avec deux cerveaux (local + Claude via l'abonnement existant).
- **Performance de premier ordre** : budgets chiffrés et mesurés (§9).
- Construire **le projet entier** d'un bloc : l'utilisateur ne veut pas de livraisons intermédiaires à valider ; le découpage interne reste libre.

### Non-objectifs

- Pas un client mail complet (pas de gestion de dossiers, filtres serveur, signatures avancées…) : lire, trier, résumer, répondre, archiver, supprimer.
- Pas de remplacement de Spotify : piloter la lecture et les playlists, pas de bibliothèque complète.
- Pas d'outil d'administration du VPS : c'est le rôle de Zenytt (`Projets/zenytt`). Oniris affiche seulement l'état, en lecture seule.
- Pas de macOS / Linux.
- Pas de version mobile.
- Pas de multi-utilisateur, pas de compte en ligne, pas de serveur Oniris.

### Anti-objectifs (échecs même si « ça marche »)

- **Coût mensuel supplémentaire.** Pas d'API Claude facturée à l'usage, pas d'abonnement à un service tiers. Refus explicite de l'utilisateur.
- **Une app qui pèse** : ventilateurs qui tournent, VRAM occupée pendant un jeu, panneau qui saccade, RAM qui grimpe au fil des heures.
- **Une interface « sans âme »** : la première maquette, une simple grille de tuiles plates, a été rejetée pour ce motif. Le niveau attendu est celui de la maquette validée.
- **Un assistant qui agit dans le dos** : envoyer, supprimer ou exécuter sans confirmation (§7.5).
- **Un assistant bavard ou lent** pour les actions simples.

## 3. Contraintes

- **Plateforme** : Windows 11 Pro uniquement.
- **Machine cible** : AMD Ryzen 9 7900X (12 cœurs), **AMD Radeon RX 7800 XT 16 Go** (pas de NVIDIA, donc pas de CUDA : l'accélération GPU passe par ROCm/HIP ou Vulkan), 32 Go de RAM. Claude Code 2.x est installé.
- **Coût** : zéro euro supplémentaire par mois. Services gratuits uniquement. Claude passe par l'abonnement existant de l'utilisateur, via Claude Code.
- **Comptes** : mails sur **Gmail seulement** (plusieurs comptes Gmail, nombre non figé), **Spotify Premium**. *Décision du 2026-11-05 : Outlook est abandonné, l'utilisateur n'a pas d'adresse Outlook ; son adresse Orange est transférée vers Gmail.*
- **Langue** : interface et voix en français.
- **Nom** : l'app s'appelle **Oniris**, l'assistante s'appelle **IRIS** (« Intelligent Responsive Interface System »). Oniris n'a été vérifié que par une recherche web : aucun logiciel homonyme, mais le nom est porté par l'école vétérinaire de Nantes et par une petite marque française de produits pour le sommeil. Si l'app est publiée un jour, il faudra vérifier les marques déposées INPI/EUIPO (classes 9 et 42).
- **Publication possible un jour** : rien n'est prévu pour l'instant, mais il ne faut pas fermer la porte. Concrètement : aucun identifiant ni secret personnel dans le code, documents juridiques (confidentialité) tenus à jour dès qu'une fonctionnalité transmet des données à un tiers (habitude de l'utilisateur sur ses autres projets).
- **Règles de travail de l'utilisateur** (son CLAUDE.md global) : commits Conventional Commits, sans co-signature Claude ; ne pas lancer de build release (`tauri build`) sans son accord ; ne pas committer ni pousser sans sa validation ; dans le dossier utilisateur, `~/pnpm-workspace.yaml` capture tout nouveau projet pnpm, donc il faut créer un `pnpm-workspace.yaml` local avant le premier `pnpm install`.

## 4. Expérience : la pilule et le panneau

L'utilisateur a choisi un mélange de la pilule permanente et du panneau qui descend du haut. Le mode plein écran séparé a été écarté.

### 4.1 La pilule

- Capsule en verre dépoli, centrée en haut de l'écran principal, toujours au premier plan.
- Elle affiche : l'heure, un égaliseur animé et le titre en cours quand Spotify joue, le nombre de mails « à traiter », et une mini-sphère IRIS qui reflète l'état de l'assistante (au repos, écoute, réflexion, parole).
- **[choix de conception]** Elle se masque automatiquement quand une application est en plein écran (jeu, vidéo). Le raccourci global continue d'ouvrir le panneau.
- **[choix de conception]** Écran principal seulement. L'écran de la pilule est réglable dans les paramètres.

### 4.2 Le panneau

- Il s'ouvre au clic sur la pilule, au survol prolongé du haut de l'écran (réglable) ou par raccourci global (**Alt+Espace** par défaut, réglable).
- La pilule se transforme en panneau qui glisse vers le bas. Le panneau occupe la largeur utile (environ 1380 px maximum) et la hauteur nécessaire, jusqu'à presque tout l'écran. Les fenêtres restent visibles autour et derrière, sous un léger voile.
- Il se ferme avec Échap, un clic en dehors ou le même raccourci. Il revient alors à l'état de pilule.
- Si le contenu dépasse, le panneau défile en interne. Il n'y a pas de second mode d'affichage.
- **Tout est utilisable au clavier** : la barre de commande a le focus à l'ouverture, Tab et les flèches permettent de naviguer entre les cartes, et chaque module a des raccourcis (exemple : Ctrl+P pour basculer de projet, déjà présent dans la maquette).

### 4.3 La voix depuis n'importe où

- **Maintenir Alt+Espace** (push-to-talk) : on parle, on relâche, et IRIS traite la demande. Un appui bref ouvre ou ferme simplement le panneau.
- **[choix de conception]** Mot d'activation « IRIS » en option, **désactivé par défaut** : il implique une écoute continue (coût CPU et vie privée). Quand il est activé, il ne doit déclencher aucune transcription, ni rien envoyer, avant la détection. Aucun modèle prêt à l'emploi ne reconnaît « IRIS » en français : il faut entraîner un modèle sur mesure, gratuitement (par exemple openWakeWord à partir d'échantillons synthétiques générés avec Piper). C'est un chantier de recherche à part, placé **en dernier**. Le push-to-talk suffit à rendre IRIS complète sans lui. Les solutions qui demandent un compte et une clé d'éditeur (Porcupine) sont exclues.
- **[décision du 2026-11-08]** Le mot d'activation « IRIS » est **reporté à un plan 8 optionnel de recherche** : le push-to-talk suffit, et l'entraînement du modèle (Python, échantillons synthétiques, réseau) n'est pas faisable dans le plan 6. La feuille IRIS affiche « Mot d'activation : bientôt », désactivé.
- Pendant l'écoute, la sphère s'anime au rythme de la voix. Une commande simple s'exécute sans ouvrir le panneau, avec une confirmation discrète dans la pilule (« Playlist Focus lancée »). Une réponse longue ouvre le panneau.

### 4.4 Démarrage et présence

- **[choix de conception]** L'app démarre avec Windows (désactivable), dispose d'une icône dans la zone de notification (paramètres, pause de la voix, quitter) et ne tourne qu'en une seule instance.

## 5. Direction visuelle

**Référence validée : [`docs/design/maquette-panneau.html`](../../design/maquette-panneau.html)** (à ouvrir dans un navigateur ; capture : `maquette-panneau.png`). L'utilisateur l'a jugée « incroyable ». Elle sert de cible visuelle et d'ambiance, pas de code à reprendre tel quel. Ses données sont fictives.

Traits à conserver :

- **Esprit Apple** : verre sombre très flouté et saturé (« vibrancy »), coins très arrondis, typographie Segoe UI Variable (Display pour les titres, chiffres tabulaires pour les heures), hiérarchie par graisse et opacité plutôt que par couleurs.
- **Accent orange corail** (`#ff7448`) et couleurs système façon iOS pour les états : bleu, vert, jaune, violet.
- **La sphère IRIS** : un anneau conique multicolore en rotation lente avec un halo qui « respire ». Son animation traduit l'état de l'assistante.
- **Le briefing** : bandeau mis en avant, texte qui s'écrit, mots clés soulignés, puis boutons d'action qui apparaissent.
- **La lumière ambiante** : des taches de couleur floues derrière le panneau, dont les couleurs viennent de la pochette en cours de lecture.
- **Le mouvement** : ouverture du panneau avec une courbe de type ressort, cartes qui montent en cascade, survols qui soulèvent légèrement. `prefers-reduced-motion` est respecté.

L'utilisateur a prévenu qu'il y aura « peut-être des éléments à améliorer ». Les ajustements de détail se feront sur l'app réelle.

## 6. Modules

Chaque module occupe une carte du panneau, alimente le briefing quand c'est pertinent, et expose ses actions à IRIS (§7.4).

### 6.1 En-tête et briefing IRIS

- En-tête : salutation selon l'heure (« Bonjour/Bonsoir Nathan »), date, sphère IRIS avec son état et le cerveau actif (local / Claude), horloge avec secondes, météo.
- **Briefing** : deux ou trois phrases qui résument ce qui compte maintenant (mails à traiter, prochain événement, projet en cours et étape du plan, alerte météo ou rappel). Il est suivi de deux ou trois **actions proposées** en un clic (« Préparer la réponse à Dupont », « Reprendre MyEmpire · orchestrateur », « Décaler le sport »).
- **[choix de conception]** Il est généré à la première ouverture de la journée, puis rafraîchi quand l'état change de façon significative (nouveau mail à traiter, événement imminent), au plus une fois toutes les 15 minutes environ, et jamais quand le panneau est fermé. Le cerveau local suffit pour ce travail.

### 6.2 Frise du jour, agenda, rappels

- **Frise horizontale** de 8 h à 23 h (bornes réglables) : événements en blocs colorés, rappels en points, repère « maintenant » qui avance.
- **Mini-calendrier mensuel** avec des points sous les jours qui ont des événements. Un clic sur un jour affiche ses événements.
- **Rappels** : liste avec cases à cocher, échéance, **notification Windows** à l'heure dite, même panneau fermé. Ils se créent à la voix ou à l'écrit en langage naturel (« rappelle-moi d'appeler la banque demain à 10 h », « dans 40 minutes »).
- **Source des événements** : l'utilisateur n'utilise pas d'agenda aujourd'hui. Les événements et rappels sont donc **stockés localement** dans Oniris. **[choix de conception]** Les agendas Google des comptes mail déjà connectés peuvent s'afficher en plus (lecture seule ; Outlook abandonné le 2026-11-05). Les comptes étant déjà autorisés pour les mails, l'autorisation agenda se demande au même moment.

### 6.3 Mails unifiés

- **Comptes** : Gmail seulement, en nombre quelconque (Outlook abandonné le 2026-11-05 ; l'adresse Orange passe par un transfert vers Gmail). Chaque compte a sa couleur, qui apparaît en pastille sur l'avatar de l'expéditeur.
- **[choix de conception]** Accès par l'API officielle, la **Gmail API**, avec OAuth (synchronisation incrémentale par historyId). Microsoft Graph n'est plus prévu depuis le 2026-11-05. Les jetons sont stockés dans le gestionnaire d'identifiants Windows.
- **Tri par IRIS** en trois onglets :
  - **À traiter** : demande une action ou une réponse de l'utilisateur ;
  - **À lire** : informatif et personnel ;
  - **Filtrés** : newsletters, notifications automatiques, publicités.

  Chaque mail affiche un **résumé d'une ligne** et, si besoin, une étiquette d'échéance (« Aujourd'hui ») ou de nature (« Bug », « Facture »).
- **Actions** :
  - ouvrir et lire le mail dans le panneau (rendu HTML isolé, images distantes bloquées par défaut) ;
  - **« Répondre avec IRIS »** : brouillon rédigé par Claude, modifiable, **envoyé seulement après confirmation explicite** ;
  - répondre à la main ;
  - archiver ;
  - supprimer (avec confirmation) ;
  - marquer comme lu ;
  - ouvrir dans le webmail.
- **Relève** : par synchronisation incrémentale toutes les 2 à 5 minutes, sans tenir de connexion permanente.
- **Applications OAuth [choix de conception]** : usage personnel, donc l'utilisateur enregistre lui-même, une fois, ses propres applications développeur (Google Cloud, Spotify for Developers). Oniris le guide pas à pas au premier lancement et stocke les identifiants client localement, jamais dans le code. L'application Google est passée au statut **« En production » sans vérification** : un écran « application non validée » est à accepter une fois par compte, mais **les jetons n'expirent pas chaque semaine** (cette expiration ne concerne que le statut « Test »). La vérification Google, payante pour les scopes Gmail restreints, n'est à envisager qu'en cas de publication ; il faudrait alors prévoir IMAP + mot de passe d'application pour Gmail.
- **[choix de conception]** Le tri est fait par le **cerveau local**, parce que c'est rapide et que le contenu des mails ne quitte pas le PC. Seuls les gestes qui demandent de la rédaction ou un vrai raisonnement passent par Claude (réponse, « résume-moi tout en détail »). Il faut l'indiquer clairement, car le contenu du mail est alors transmis à Anthropic via Claude Code.

### 6.4 Musique (Spotify Premium)

- La carte « En lecture » affiche la pochette, le titre, l'artiste, une forme d'onde animée, une barre de progression déplaçable, les boutons précédent, lecture/pause et suivant, ainsi que **les playlists favorites** en vignettes.
- **[choix de conception]** Deux canaux sont utilisés :
  - **Windows Media Session (SMTC)** pour l'état « en cours » : instantané, sans requête réseau, il fonctionne même si l'app Spotify est hors ligne ;
  - la **Spotify Web API** (OAuth PKCE, compte Premium) pour lancer une playlist, chercher et lancer un titre, choisir l'appareil, gérer le volume et la file d'attente.
- **À la voix** : « mets ma playlist Focus », « mets du Daft Punk », « suivant », « moins fort », « c'est quoi ce titre ? ».
- La pochette fournit les couleurs de la lumière ambiante du panneau (couleurs dominantes extraites).
- **Restrictions Spotify de février 2026 (mode développement, décision du 2026-11-05)** : le propriétaire de l'application doit être Premium (c'est le cas) ; cinq utilisateurs au plus (largement suffisant) ; la recherche est limitée à `limit=10`. Restent disponibles : `/me/player/*`, `/me/playlists`, `/playlists/{id}/items` et `/search`. À ne **pas** utiliser : `/playlists/{id}/tracks`, les lots (`GET /tracks?ids=`), `/browse/*` et `/artists/{id}/top-tracks`.

### 6.5 Favoris

- Une grille d'icônes façon dock (favicon ou initiales sur un fond coloré), regroupable par catégories, réordonnable par glisser-déposer.
- **Import** : depuis l'export JSON de Nexus_Dashboard, qui sait déjà exporter ses dashboards en JSON, et depuis les favoris des navigateurs installés.
- Ils sont accessibles à la voix (« ouvre GitHub ») et depuis la recherche.

### 6.6 Recherche universelle (barre de commande)

- Une seule barre, en bas du panneau, qui sert à la fois à **parler à IRIS** et à **chercher**. Le texte libre est interprété (commande, question ou recherche), et des résultats instantanés s'affichent pendant la frappe.
- **Ce qu'on peut trouver** : fichiers et dossiers, projets, applications installées, favoris, mails (objet et expéditeur), rappels, notes rapides, commandes Oniris (« / » liste les commandes).
- **Objectif** : moins de 50 ms entre la frappe et l'affichage des résultats. Recherche floue avec tolérance aux fautes, classée selon l'usage (récence et fréquence d'ouverture).
- **[choix de conception]** Un **index de fichiers propre à Oniris**, maintenu en tâche de fond à faible priorité, surveille les changements. Racines par défaut : dossier utilisateur et `Projets`, réglables. Exclusions par défaut : `node_modules`, `target`, `.git`, caches, `AppData`. Si l'outil « Everything » est installé, il peut servir de source alternative. Le choix technique de l'indexation est laissé au constructeur, à condition de tenir le budget et de ne pas solliciter le disque en continu.

### 6.7 Projets et reprise avec Claude

- **Liste** : les sous-dossiers de `C:\Users\natha\Desktop\Utilitaires\Projets` (racine réglable, plusieurs racines possibles), triés par activité récente. On peut épingler ou masquer un projet. Le dossier `AppData` qui s'y trouve n'est pas un projet.
- **Chaque carte de projet affiche** :
  - icône/initiales, nom, dernière activité (« il y a 3 jours ») ;
  - **progression dans le plan** : s'il existe un plan de développement (cases à cocher de `docs/superpowers/plans/*.md`, à défaut un fichier de plan reconnu), on montre « étape 7/12 » dans un anneau ;
  - **« Tu en étais à… »** : le titre ou le résumé de la dernière session Claude Code du projet, lu dans les journaux locaux de Claude Code (`~/.claude/projects/<dossier encodé>/*.jsonl`), sinon l'étape en cours du plan ;
  - git : branche, nombre de fichiers modifiés non commités, activité des 14 derniers jours en petits carrés ;
  - un bouton **Reprendre** en deux variantes : **Normal** et **Orchestrateur**.
- **Reprendre** :
  1. ouvre le projet dans VS Code, ou met au premier plan la fenêtre VS Code déjà ouverte sur ce projet ;
  2. lance Claude avec une consigne de reprise, par exemple : « Reprends là où tu en étais sur ce projet. Appuie-toi sur le plan de développement et l'historique git. Commence par me dire où on en est. » La variante Orchestrateur ajoute la demande de travailler en mode orchestrateur, comme le décrit le CLAUDE.md global de l'utilisateur : graphify, sous-agents haiku/sonnet/opus selon la tâche, parallélisation. Les deux consignes sont modifiables dans les paramètres.
- **Point à vérifier en premier par le constructeur** : comment injecter la consigne **dans l'extension Claude de VS Code**, qui est l'usage actuel de l'utilisateur (gestionnaire d'URI de l'extension ou autre mécanisme). **Repli accepté** si ce n'est pas faisable proprement : ouvrir un terminal intégré dans le projet et y lancer Claude Code en ligne de commande, avec la reprise de la dernière conversation (`claude --continue`) et la consigne.
- **À la voix** : « reprends Zenytt », « reprends MyEmpire en mode orchestrateur », « où j'en suis sur MacroWorld ? ». Cette dernière question donne une réponse parlée et écrite tirée du plan, de git et de la dernière session, sans rien ouvrir.

### 6.8 VPS et sites

- Liste des sites avec pastille d'état (en ligne / lent / hors ligne), temps de réponse et petite courbe des dernières mesures. Le titre de la carte affiche le compte (« 6/6 en ligne »).
- **[choix de conception]** **Sondes HTTP(S)** vers les URL configurées, toutes les minutes, **même panneau fermé**, pour pouvoir notifier une panne. Une notification Windows part si un site tombe ou dépasse un seuil de latence.
- **Règle hors ligne (2026-11-05)** : « hors ligne » = toutes les sondes échouent sans aucune réponse HTTP **et** Windows (Network List Manager) indique qu'il n'y a pas d'Internet. Le tour est alors ignoré : pas de panne, pas de notification, pas de point dans l'historique. Sinon, un tour où tout échoue est une vraie panne (les sites étant des sous-domaines d'un même VPS, la panne du VPS les fait tous tomber).
- **En option** : charge CPU/RAM du VPS via le **serveur MCP en lecture seule de Zenytt** (`Zenytt.exe --mcp`), qui détient déjà la connexion et les secrets SSH. Oniris ne doit **pas** gérer lui-même d'identifiants SSH.
- Strictement en lecture seule. Toute action sur le VPS reste dans Zenytt. IRIS peut ouvrir Zenytt.

### 6.9 État du PC

- Jauges CPU, GPU, RAM et VRAM, avec la température CPU si elle est disponible sans pilote tiers.
- **Transparence d'Oniris sur lui-même** : état du modèle IA local (en veille / chargé, VRAM utilisée), état de la reconnaissance vocale (chargée / déchargée) et mémoire de l'app.
- Mesures prises **uniquement quand le panneau est ouvert**.

### 6.10 Météo

- Conditions actuelles et prévisions du jour dans l'en-tête, plus une alerte pluie qui alimente le briefing (« pluie à 18 h, je peux décaler le sport »).
- **[choix de conception]** **Open-Meteo** : gratuit, sans clé, ville réglable.

### 6.11 Historique du presse-papiers

- Les N derniers éléments copiés (texte, images en miniature), avec recherche, épinglage et collage en un clic.
- Les copies issues de gestionnaires de mots de passe ne sont jamais enregistrées : on respecte les formats Windows qui excluent un contenu de l'historique, comme le fait l'historique natif de Windows.
- Données locales, purge automatique (durée réglable).

### 6.12 Notes rapides

- Un bloc-notes léger : notes courtes, dictables à la voix (« IRIS, note : idée de nom pour… »), consultables et cherchables, et transformables en rappel.

### 6.13 Minuteur et focus

- Minuteurs (« minuteur 25 minutes ») et sessions de type Pomodoro. Le compte à rebours s'affiche dans la pilule.
- **[choix de conception]** En option, le lancement d'une session focus peut démarrer la playlist Focus.

## 7. IRIS, l'assistante

### 7.1 Personnalité

- Une voix **féminine**, française. Ton **chaleureux, concis, avec une pointe d'humour** discrète, à la JARVIS : jamais servile, jamais bavarde. Elle tutoie l'utilisateur et l'appelle par son prénom.
- **[choix de conception]** Le ton est réglé par un texte de personnalité modifiable dans les paramètres.

### 7.2 Entrées et sorties

- **Écrit** : la barre de commande du panneau.
- **Voix** : push-to-talk (Alt+Espace maintenu), mot d'activation en option (§4.3).
- **[décision du 2026-11-08]** Whisper tourne sur le **processeur** avec le binaire officiel de whisper.cpp (tag `b5454`, `whisper-server.exe`) : aucun binaire Windows Vulkan n'est publié et le PC n'a ni CMake ni SDK Vulkan. Modèle par défaut `small` (q5_1, 190 Mo), option `turbo` (574 Mo). L'accélération Vulkan est notée pour le plan 7, seulement si la latence mesurée dépasse 1 s. Piper `2023.11.14-2` avec `fr_FR-siwis-medium` (processeur, aucune VRAM). Les fichiers sont téléchargés sur clic, épinglés par taille et SHA-256, dans `%LOCALAPPDATA%\Oniris\voice`.
- **Reconnaissance vocale locale** avec **Whisper** (famille whisper.cpp / faster-whisper), accélérée sur le GPU AMD via Vulkan ou ROCm. Le modèle n'est chargé que pour la transcription, puis libéré après quelques minutes d'inactivité.
- **Synthèse vocale locale** avec **Piper** et une voix féminine française. Elle peut être désactivée (réponses écrites seulement) et le volume est réglable.
- **Barge-in** : si l'utilisateur reparle ou appuie sur Échap, IRIS se tait. **[décision du 2026-11-08]** Chaque appui sur le raccourci la fait taire, même voix en pause ou non installée. Le micro est coupé au bout de 30 s de maintien. Les tours vocaux s'affichent dans le panneau, et un message bref (réussite, échec) apparaît dans la pilule.

### 7.3 Deux cerveaux, zéro coût

| | Cerveau local | Claude |
|---|---|---|
| Moteur | Modèle d'environ 8 milliards de paramètres via **Ollama** sur la RX 7800 XT. Le meilleur modèle de cette taille disponible au moment du développement, bon en français et en appels d'outils. | **Claude Code en mode non interactif** (`claude -p`, sortie structurée), avec l'**abonnement Claude existant**. Aucune clé API. |
| Rôle | Comprendre les demandes, appeler les outils simples, trier et résumer brièvement les mails, générer le briefing, parler. | Rédiger (réponses aux mails), raisonner, synthèses longues, questions ouvertes, tout ce qui dépasse le cerveau local. |
| Latence visée | < 1 s de la fin de la phrase à l'action | Quelques secondes, affichées en continu (streaming) |
| Données | Ne quittent pas le PC | Envoyées à Anthropic via Claude Code |

- **Aiguillage** : le cerveau local reçoit toutes les demandes et décide lui-même s'il les traite ou s'il « passe la main » à Claude. Cette délégation est un outil comme un autre. L'utilisateur peut aussi forcer Claude (« demande à Claude… » ou un bouton). La sphère indique quel cerveau travaille.
- **Les deux cerveaux voient les mêmes outils.** **[choix de conception]** Oniris expose ses outils (§7.4) sous forme de **serveur MCP local**, fourni à Claude Code au lancement. Ainsi Claude peut, par exemple, lire un mail ou créer un rappel par le même chemin, avec les mêmes règles de confirmation.
- **Ressources** : le modèle local se charge à la première demande et se décharge après 5 minutes d'inactivité (réglable). Quand une app est en plein écran, ou qu'un jeu tourne, Oniris ne garde aucun modèle en mémoire. Ollama est une dépendance installée à part : Oniris vérifie sa présence et guide l'installation au premier lancement.
- **Si Claude est indisponible** (limites d'usage atteintes, pas de réseau) : IRIS le dit simplement et propose de réessayer plus tard, ou de faire une version simple avec le cerveau local.
- **[décision 2026-11-07, plan 5]** Modèle local par défaut : `qwen3:8b` (réglable ; `ministral-3:8b` à comparer à la main). Sans Ollama (absent, arrêté, modèle manquant), IRIS bascule sur Claude **tant qu'aucun outil n'a encore tourné dans le tour**, et le dit dans la conversation ; après un outil, elle s'arrête sur une erreur plutôt que de rejouer. Les demandes déterministes (« suivant », « rappelle-moi de… ») passent par un raccourci sans modèle (< 50 ms, test automatisé). Claude est lancé sans outils natifs (`--tools ""`) et ne voit que `mcp__oniris__*`, relayés par `oniris.exe --mcp-relay` (jeton aléatoire par tour, 127.0.0.1) ; le relais en build de production reste à vérifier au plan 7.

### 7.4 Outils d'IRIS

Chaque module expose ses actions sous forme d'outils typés. Par exemple :

- lire l'agenda ; créer, déplacer ou supprimer un événement ;
- créer, cocher ou supprimer un rappel ;
- lister, lire, résumer, archiver, supprimer des mails ; rédiger, puis envoyer une réponse ;
- lancer une playlist ou un titre ; pause ; suivant ; volume ;
- ouvrir un favori, une app, un fichier ou un dossier ; chercher ;
- lister les projets, donner l'état d'un projet, reprendre un projet (Normal / Orchestrateur) ;
- donner l'état des sites ;
- créer une note ;
- lancer un minuteur ;
- consulter l'état du PC ;
- gérer les paramètres simples d'Oniris (couper la voix, etc.).

**Outils généraux (demande explicite de l'utilisateur).** L'utilisateur veut pousser IRIS loin : elle dispose donc aussi d'outils **généraux**, en plus des outils typés.

- **exécuter une commande** (PowerShell) ;
- **gérer des fichiers** : créer, copier, renommer, déplacer, supprimer ;
- **lancer, fermer ou mettre au premier plan une application**.

Ces outils sont protégés contre l'injection de consignes (un mail piégé qui essaierait de donner des ordres à IRIS) par plusieurs couches indépendantes. **[choix de conception]** La conception exacte est à affiner par le constructeur, mais ces garanties sont obligatoires :

1. **Séparation des rôles (schéma « double LLM » / CaMeL)** : le modèle qui planifie et appelle les outils ne lit **jamais** directement un contenu non fiable (corps de mail, page web, fichier téléchargé). Ces contenus sont traités par une instance « en quarantaine », sans aucun outil, qui ne renvoie que des données structurées et bornées : résumé, catégorie, dates extraites. Elle ne renvoie jamais de texte réinjecté comme consigne.
2. **Marquage de provenance** : chaque donnée garde la trace de son origine. Un argument d'outil qui provient d'un contenu non fiable (par exemple une commande ou un chemin trouvé dans un mail) rend l'appel **systématiquement soumis à confirmation**, quel que soit l'outil, avec mention de cette origine sur la carte de confirmation.
3. **Confirmation lisible** : pour une commande, la carte affiche la commande exacte, une explication en français de ce qu'elle fait et son niveau de risque. Pour les fichiers, elle affiche la liste exacte des fichiers concernés et le résultat attendu.
4. **Liste blanche réglable** : des commandes en lecture seule (par exemple `git status`, `dir`, `Get-Process`) et des dossiers de travail peuvent être autorisés sans confirmation **seulement si la demande vient directement de l'utilisateur**.
5. **Garde-fous fixes**, impossibles à désactiver depuis l'assistant : pas d'élévation administrateur, dossiers système et `%APPDATA%\Oniris` protégés en écriture, suppressions envoyées dans la **Corbeille** (jamais de suppression définitive), délai d'exécution maximal, sortie tronquée.
6. **Journal d'actions** : toute action d'outil est journalisée (qui l'a demandée, provenance, commande, résultat), consultable dans le panneau, avec **annulation** quand c'est possible (fichier restauré depuis la Corbeille, renommage inversé).
7. **Claude passe par les mêmes portes** : quand Claude Code est lancé en mode non interactif pour IRIS, ses outils natifs (Bash, Edit, Write…) sont désactivés. Il n'agit que via le serveur MCP d'Oniris, donc avec les mêmes confirmations, la même provenance et le même journal.

Ces protections doivent être couvertes par des tests dédiés, en particulier des mails piégés de test qui tentent de déclencher une commande. Le vrai travail de code sur un projet reste dans VS Code via « Reprendre ».

**[décision 2026-11-07, plan 5]** Réalisation des garanties ci-dessus :

- **Références `{{uN}}`** : tout contenu non fiable lu par un outil (mail, page, fichier) est rangé dans un coffre par conversation ; le planificateur (local ou Claude) n'en voit que des références `{{u1}}`, `{{u2}}`… et jamais le texte. Un argument d'outil qui contient une référence rend l'appel soumis à confirmation, avec la source sur la carte. Les valeurs de forme identifiant (sans espace, 100 caractères au plus) restent en clair : un jeton choisi par un tiers mais de cette forme peut donc être réutilisé sans carte pour un outil libre, risque accepté.
- **Liste blanche de commandes** réduite par défaut à `git status`, `Get-ChildItem`, `Get-Location`, `Get-Date` (pas de `git log` ni `git branch`, qui peuvent écrire) ; vérification mot à mot, rejet des commandes encodées, des chemins UNC et des jetons d'élévation. Les entrées ajoutées par l'utilisateur sont à sa charge.
- **Fichiers** : en plus de `%APPDATA%\Oniris` et des dossiers système, les dossiers Démarrage et les profils PowerShell sont protégés en écriture, et `files.copy` ne lit pas `%APPDATA%\Oniris`. La suppression passe par la Corbeille avec l'avertissement Windows (`FOF_WANTNUKEWARNING`) : ce qui ne peut pas y aller (disque amovible) déclenche une question de Windows, et un refus fait échouer l'outil plutôt que de supprimer pour de bon.
- **Journal** : la commande exacte et les chemins y sont gardés (§7.4.6) ; les journaux techniques n'en contiennent jamais.

**Ajouter un outil doit rester simple.** Un module = un ensemble d'outils déclarés au même endroit, avec leur niveau d'autorisation.

### 7.5 Ce qu'IRIS fait seule, et ce qu'elle demande d'abord

Règle validée par l'utilisateur :

- **Libre** : tout ce qui lit, ouvre ou s'annule facilement. Musique, ouvrir un projet, un fichier ou un favori, chercher, créer ou cocher un rappel, une note ou un minuteur, résumer des mails, reprendre un projet.
- **Confirmation obligatoire** : tout ce qui sort du PC ou ne se rattrape pas. Envoyer un mail, supprimer un mail, supprimer un événement, exécuter une commande (hors liste blanche), supprimer, déplacer ou écraser des fichiers (hors dossiers autorisés), et **toute action dont un argument provient d'un contenu non fiable** (voir les garanties du §7.4). Les actions sur le VPS restent hors d'Oniris : c'est le rôle de Zenytt.

La confirmation est **imposée par la couche outils d'Oniris, pas par le modèle** : un outil « à confirmer » ne s'exécute qu'après un clic ou un « oui » explicite de l'utilisateur sur une carte qui montre exactement ce qui va être fait (destinataire et texte du mail, fichiers concernés…). Le même mécanisme s'applique quand l'appel vient de Claude via le serveur MCP.

### 7.6 Mémoire

- **Historique des conversations** conservé localement, consultable et effaçable.
- **[choix de conception]** Une mémoire de **préférences explicites** (« retiens que ma playlist de sport c'est X »), visible et modifiable dans les paramètres. Pas de profilage implicite.

## 8. Architecture technique

**[choix de conception]** La pile est celle que l'utilisateur connaît déjà par Zenytt et MacroWorld, pour réutiliser ses habitudes et ses outils :

- **Tauri 2** : cœur en **Rust**, interface web dans **WebView2**.
- **Interface** : **React + TypeScript + Vite**, avec discipline de performance : animations uniquement en CSS sur `transform` et `opacity`, pas de re-rendu global à chaque tic d'horloge, état découpé par module. Le constructeur peut choisir un framework plus léger (Solid, Svelte) s'il le justifie par des mesures.
- **Cœur Rust découpé en modules** : agenda/rappels, mails, Spotify, recherche/index, projets, sondes, système, presse-papiers, assistant (orchestration des cerveaux, outils, voix), serveur MCP. Chaque module a son interface claire et peut être testé seul.
- **Stockage** : **SQLite** dans `%APPDATA%\Oniris`. **Secrets** (jetons OAuth) dans le **gestionnaire d'identifiants Windows**, jamais dans un fichier.

  **Décision : SQLite plutôt que PostgreSQL** (discutée et acceptée par l'utilisateur).
  - **Pourquoi une base** : il faut conserver l'état de synchronisation et le cache des mails avec leur tri et leurs résumés, l'agenda, les rappels, les notes, le presse-papiers, les conversations, le journal d'actions avec annulation, l'historique des sondes, les statistiques d'usage et les paramètres. Des fichiers JSON n'offrent ni index, ni écriture atomique, ni recherche plein texte.
  - **Pourquoi pas PostgreSQL** : c'est un serveur résident, qui coûte de la RAM et des réveils CPU (autovacuum, checkpoints), contre l'impératif de légèreté. Chaque requête passe par une connexion locale (≈ 0,1 à 0,5 ms), contre quelques microsecondes pour SQLite qui tourne dans le processus. Il impose aussi un service à installer, mettre à jour et sauvegarder, ce qui est rédhibitoire en cas de publication. Et ses atouts (écrivains concurrents, accès réseau) sont inutiles pour un seul utilisateur et une seule app.
  - **Conditions d'emploi de SQLite** :
    - mode **WAL** ;
    - **un seul processus propriétaire de la base** : le serveur MCP relaie vers l'app et n'ouvre jamais la base lui-même ;
    - tables **STRICT** ;
    - migrations versionnées ;
    - recherche plein texte **FTS5**, avec racinisation française faite côté Rust ;
    - mémoire sémantique éventuelle via **sqlite-vec**, ou une bibliothèque vectorielle embarquée si ça ne suffit pas.
  - **L'index de fichiers n'est pas en base** : c'est une structure en mémoire faite pour la recherche floue en moins de 50 ms, avec une copie sur disque pour redémarrer vite.
  - **Quand changer** : si un jour Oniris se synchronise entre plusieurs appareils, un serveur de synchronisation avec PostgreSQL vivra sur le VPS ; SQLite restera le cache local du PC.
- **Processus externes** : Ollama (service local), whisper et Piper (embarqués ou téléchargés au premier lancement), Claude Code (CLI déjà installée).
- **Fenêtres** : la façon de réaliser la pilule et le panneau (une fenêtre transparente redimensionnée ou deux fenêtres, gestion du clic à travers les zones transparentes, détection du plein écran) est laissée au constructeur. Les budgets du §9 et la fluidité de l'ouverture priment.
- **Raccourcis globaux, notifications Windows, démarrage automatique, instance unique** : via les plugins Tauri existants quand ils conviennent.
- **Hors ligne** : tout ce qui est local reste utilisable (recherche, projets, rappels, notes, cerveau local, voix). Les modules en ligne affichent leur dernier état connu.

## 9. Performance : budgets mesurés

Les budgets se vérifient avec des mesures reproductibles (script ou test) avant de déclarer un module terminé.

| Mesure | Budget |
|---|---|
| Ouverture du panneau (raccourci → panneau complet affiché) | < 100 ms, animation fluide à la fréquence de l'écran |
| Recherche (frappe → résultats) | < 50 ms |
| Commande vocale simple (fin de phrase → action) | < 1 s, modèle déjà chargé |
| Premier mot de la réponse Claude | affichage du streaming dès réception |
| CPU, panneau fermé, au repos | ≈ 0 % (< 0,5 % en moyenne) |
| RAM totale, panneau fermé (Oniris + WebView2) | le plus bas possible **sans jamais dégrader l'ouverture** ; repère < 200 Mo, mesuré dès le premier prototype de fenêtre |
| VRAM, modèles en veille | 0 Go |
| Fuite mémoire | aucune croissance sur 24 h d'usage normal |

**Priorité tranchée par l'utilisateur : l'ouverture instantanée est impérative.** L'interface reste donc chargée en permanence, et la RAM s'optimise sans jamais sacrifier ce point. Pistes à mesurer : interface pré-rendue et seulement masquée, framework d'interface léger, flou limité aux surfaces visibles, limitation de la mémoire de WebView2, rendu paresseux des cartes hors écran. L'estimation de 30 à 60 Mo donnée pendant la discussion ne comptait que le cœur Tauri, sans les processus WebView2. Le choix du framework d'interface (§8) doit être validé par une mesure d'ouverture et de RAM dès le premier prototype, avant de construire les modules.

Principes :

- rien ne tourne pour rien (pas d'animation ni de mesure quand le panneau est fermé) ;
- relèves réseau espacées et incrémentales ;
- index mis à jour par événements ;
- modèles chargés à la demande.

## 10. Confidentialité et données

- **Local d'abord** : tout est stocké sur le PC. Pas de télémétrie.
- **Données qui sortent du PC**, et seulement dans ces cas : appels aux API Google (Gmail et Agenda) et Spotify (comptes de l'utilisateur), Open-Meteo (ville), sondes HTTP vers ses propres sites, et **Claude via Claude Code** quand ce cerveau est utilisé (contenu de la demande, et des mails concernés le cas échéant).
- Un fichier `docs/legal/confidentialite.md` décrit ces flux dès le départ et suit les ajouts. C'est une habitude de l'utilisateur, utile en cas de publication.

## 11. Qualité et tests

- Tests unitaires Rust par module, tests de l'interface (Vitest), et un parcours de bout en bout piloté par le protocole de débogage de WebView2. L'utilisateur a déjà cet outillage sur ses autres projets Tauri.
- Les intégrations externes (Gmail, Spotify, Claude Code, Ollama) passent par des interfaces simulables dans les tests.
- Les budgets du §9 font partie des critères de fin.
- **Aucun test ne doit envoyer de vrai mail, supprimer de vraies données ni agir sur le vrai VPS.**

## 12. Ce qui est laissé au constructeur

- **[décision du 2026-11-08, plan 7]** Le démarrage automatique n'est écrit dans la clé `Run` qu'après le consentement donné une fois au premier lancement. La version est **1.0.0**. Le panneau n'est pas zoomé pour tenir en 1080p : l'écran de l'utilisateur est en 2560×1440 (cible de la spec) et un zoom ramènerait le texte à environ 10 px ; en 1080p la grille défile, rien n'est caché. L'installeur NSIS est par utilisateur, en français, et garde les dossiers de données à la désinstallation.

- Le découpage en étapes de travail et leur ordre : l'utilisateur veut le projet complet, sans livraisons intermédiaires, mais la construction peut être séquencée librement.
- Les choix de bibliothèques précis (index de fichiers, extraction de couleurs, client OAuth, liaison Whisper et Piper, etc.), dans le respect des budgets et du coût nul.
- Le choix exact des modèles (LLM local, taille du modèle Whisper, voix Piper féminine française), avec des mesures de latence et de qualité en français à l'appui.
- La mise en œuvre de la pilule et du panneau (fenêtrage, transparence, clic à travers).
- Le mécanisme d'injection de la consigne dans l'extension Claude de VS Code (§6.7), avec le repli prévu.
- Le contenu exact des écrans de paramètres et du premier lancement (connexion des comptes, installation d'Ollama, choix des racines de projets et de recherche).
- La gestion du dépôt git : le dossier n'est pas encore versionné, et l'utilisateur doit donner son accord avant tout `git init`.
