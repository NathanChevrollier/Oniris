# Confidentialité d'Oniris

Version 9 — 12 novembre 2026. Ce document décrit simplement ce qu'Oniris fait de tes données. Il est mis à jour à chaque nouveauté qui touche à ta vie privée.

## En bref

Oniris range tout chez toi, sur ton PC. Il n'envoie aucune statistique d'usage, et il n'y a aucun compte à créer pour l'utiliser. Oniris ne contacte Internet que pour ce que tu as choisi d'activer : la météo (si tu as choisi une ville), les icônes de tes favoris, les sondes de tes sites, le téléchargement de la voix (sur ton clic) et, si tu connectes tes comptes, Google (Gmail et Agenda) et Spotify. Les comptes ne sont jamais obligatoires : sans eux, rien de tout cela n'est contacté. IRIS, l'assistante, réfléchit d'abord **sur ton PC** ; elle ne passe par Claude (donc par Anthropic) que dans les cas décrits plus bas.

## Où sont tes données

Tout est stocké en local, dans le dossier `%APPDATA%\Oniris` :

- la base de données `oniris.db` : réglages, agenda, rappels, notes, minuteurs, historique du presse-papiers, journal des actions, favoris, statistiques d'usage de la recherche, préférences de projets, liste de tes comptes (adresse et couleur, jamais les jetons), cache des en-têtes de mails, cache des événements Google, mesures des sondes, conversations avec IRIS, préférences qu'elle retient et résumés de tri des mails ;
- l'instantané de l'index de recherche `index.bin` : la liste des **chemins complets** de tous les fichiers et dossiers indexés (voir « Recherche de fichiers ») ;
- les journaux techniques (`logs`) : ils ne contiennent ni le texte de tes notes, ni celui de tes rappels, ni celui de ton presse-papiers, ni adresse mail, sujet, titre d'événement ou de morceau, ni jeton. Seulement des identifiants, des compteurs, des codes de réponse et des messages d'erreur.

- les fichiers de la voix, dans un dossier à part, `%LOCALAPPDATA%\Oniris\voice` : les programmes Whisper et Piper et leurs modèles (de l'ordre de 300 Mo avec le modèle rapide, 700 Mo avec le précis). Ce ne sont pas des données personnelles ; supprimer ce dossier retire la voix, que tu peux réinstaller ensuite.

Supprimer le dossier `%APPDATA%\Oniris` efface tout ce qu'Oniris sait de toi.

## Ce que garde la base

- **Suppression d'une note, d'un rappel ou d'un événement** : pour l'instant, Oniris ne fait que le masquer. Le texte reste dans `oniris.db` jusqu'à une purge, prévue dans une version ultérieure. Seul le fait de supprimer le dossier `%APPDATA%\Oniris` l'efface aujourd'hui à coup sûr.
- **Journal des actions** : chaque action (créer une note, cocher un rappel, lancer un minuteur…) y est enregistrée avec ses arguments, y compris le texte d'une note, d'un rappel ou d'un événement que tu crées toi-même. Ce journal n'est pas encore purgé automatiquement. Deux exceptions : le sujet et le texte d'un mail à envoyer n'y figurent jamais (seulement le nombre de caractères), et le résultat d'une lecture de mails ou d'événements (par exemple les titres des événements d'un agenda Google) n'y est pas gardé, seulement le nombre d'éléments.
- **Recherche dans le presse-papiers** : le journal garde les mots cherchés et le nombre de résultats, jamais les éléments trouvés.

## Notifications

Le texte de tes rappels et le nom de tes minuteurs s'affichent dans les notifications de Windows. Windows les garde ensuite dans le centre de notifications jusqu'à ce que tu les effaces : toute personne qui a accès à ta session peut donc les lire.

## Pas de télémétrie

Oniris ne mesure pas ton usage et n'envoie ni rapport de plantage, ni statistique, ni identifiant à qui que ce soit.

## Météo

Si tu laisses le champ « ville » vide, la météo est désactivée et Oniris ne fait aucune requête.

Si tu choisis une ville, Oniris contacte le service gratuit [Open-Meteo](https://open-meteo.com) (sans compte ni clé) :

- il envoie le nom de la ville que tu as saisi pour trouver ses coordonnées, puis les coordonnées (latitude et longitude) pour obtenir la prévision ;
- ces échanges se font au plus une fois toutes les 30 minutes environ, quand tu ouvres le panneau, et aussi juste après chaque changement de ville ;
- après un échec (réseau coupé, ville introuvable), Oniris attend 10 minutes avant de réessayer, toujours à l'ouverture du panneau ;
- Open-Meteo voit, comme tout site web, l'adresse IP de ta connexion ;
- Oniris n'envoie ni ton nom, ni ton adresse e-mail, et s'identifie seulement par « Oniris/0.1 ».

Pour arrêter, vide le champ « ville » : le lieu et la dernière météo mémorisés sont effacés.

## Favoris

Tes favoris restent sur ton PC, dans la base d'Oniris.

- **Import** : seulement quand tu cliques sur « Importer ». « Depuis les navigateurs » lit en local les signets de Chrome, Edge, Brave et Firefox (Firefox : sur une copie temporaire, supprimée aussitôt) ; « Depuis un export Nexus… » lit le fichier JSON que tu choisis. Rien n'est envoyé ailleurs, et les liens dangereux (par exemple `javascript:`) sont ignorés.
- **Icônes** : pour afficher l'icône d'un site, Oniris envoie **quelques requêtes vers ce site, ou vers l'adresse d'icône qu'il indique (parfois un autre domaine)**, au plus une par seconde : d'abord `/favicon.ico`, puis, si besoin, sa page d'accueil pour y trouver son icône. Aucun service tiers n'est utilisé, aucun cookie n'est envoyé et Oniris s'identifie seulement par « Oniris/0.1 ». Le site voit, comme tout site web, l'adresse IP de ta connexion, et ton résolveur DNS voit aussi ces noms de domaine. Ces requêtes ont lieu après un import, un ajout et au démarrage ; l'icône est réduite et gardée sur ton PC. En cas d'échec, Oniris garde les initiales et ne réessaie pas avant 7 jours. Oniris refuse de contacter une adresse de ton réseau local.

## Recherche de fichiers

Pour te répondre vite, Oniris construit une liste des **noms et des chemins** de tes fichiers et dossiers, dans les dossiers que tu as choisis (par défaut, ton dossier utilisateur), plus ceux de tes dossiers de projets.

- Oniris **n'ouvre jamais le contenu de tes fichiers**. Il ne lit que leurs noms, leur emplacement et leurs propriétés de base. Une seule exception : les fichiers `.gitignore`, de petits fichiers de réglage lus pour ne pas indexer les dossiers de compilation.
- Cette liste vit en mémoire et est aussi **enregistrée sur ton disque**, dans `%APPDATA%\Oniris\index.bin` : un instantané des chemins complets de tout ce qui est indexé (il contient donc les noms de tes dossiers et fichiers), pour que la recherche réponde dès le démarrage. Il est réécrit à la fin de chaque parcours complet du disque, puis toutes les 10 minutes environ si quelque chose a changé. Il disparaît avec le dossier `%APPDATA%\Oniris`.
- Les fichiers OneDrive « à la demande » (marqués « à télécharger », icône de nuage) ne sont **pas indexés** : Oniris ne les liste pas et ne déclenche aucun téléchargement.
- La base garde seulement des **statistiques d'usage** : le type et le chemin de ce que tu as ouvert depuis la recherche, un score et la date du dernier usage, pour classer en premier ce que tu utilises souvent. Ces statistiques ne sont pas encore purgées automatiquement ; supprimer le dossier `%APPDATA%\Oniris` les efface.
- Les journaux techniques ne contiennent jamais de chemin : seulement des compteurs.
- La recherche n'envoie rien sur Internet.

## Projets

La carte Projets lit **en local** les dossiers de projets que tu as choisis :

- **Git** : Oniris lance `git status` et `git log` (sur les 14 derniers jours, dates seulement) dans chaque projet, pour afficher la branche, le nombre de fichiers modifiés et l'activité récente. Aucune commande n'écrit dans ton dépôt et rien n'est envoyé.
- **Plan** : si le projet contient un plan d'implémentation (`docs/superpowers/plans/*.md`), Oniris en lit les cases cochées et le titre de la tâche en cours.
- **Sessions Claude Code** : Oniris regarde dans `~/.claude/projects` la session la plus récente du projet (sa date et son identifiant). Pour afficher un titre, il lit au plus les 256 derniers Ko de ce fichier, jamais le fichier entier, et n'en garde que 80 caractères, **en mémoire seulement**. Ce titre peut être un résumé ou ton premier message de la fenêtre lue. Il n'est pas enregistré dans la base.
- **Reprendre** : ce bouton ouvre VS Code sur le projet puis lance Claude, sur ton PC, avec une consigne qui peut contenir ce titre. Oniris ne transmet rien à un serveur ; c'est Claude Code qui communique ensuite avec Anthropic, comme quand tu le lances toi-même.
- La base garde seulement ce que tu choisis : projet épinglé ou masqué (son chemin).

## Historique du presse-papiers

L'historique reste sur ton PC ; il n'est envoyé nulle part. Ses limites :

- les copies faites depuis un gestionnaire de mots de passe qui les signale comme sensibles sont exclues ;
- en revanche, les **extensions de gestionnaires de mots de passe dans le navigateur ne signalent pas leurs copies** : un mot de passe copié depuis une telle extension peut donc être enregistré dans l'historique ;
- tu peux désactiver l'historique à tout moment : clic droit sur l'icône d'Oniris dans la zone de notification, puis décoche « Historique du presse-papiers ». Plus rien n'est lu ni enregistré ensuite. Les éléments déjà gardés restent jusqu'à ce que tu les supprimes ou que la purge les retire ;
- il est purgé automatiquement, au démarrage d'Oniris et après chaque copie : les éléments plus vieux que la durée choisie (7 jours par défaut) et ceux au-delà du nombre maximal (200 par défaut) sont supprimés, sauf ceux que tu as épinglés. Ici, la suppression est réelle : l'élément est effacé de la base.

## Comptes et jetons

Connecter un compte est un choix : rien n'est contacté tant que tu n'as pas cliqué sur « Connecter » dans la feuille « Comptes ».

- **Tes propres applications** : tu crées toi-même, une fois, ton application Google (Google Cloud) et ton application Spotify (Spotify for Developers), en suivant le guide d'Oniris. Aucun identifiant n'est écrit dans le programme. L'identifiant client est gardé dans les réglages ; le secret client Google, lui, est dans le coffre Windows.
- **Connexion** : elle se fait dans ton navigateur, sur le site de Google ou de Spotify. Oniris n'a **jamais ton mot de passe**. Il écoute un instant sur ton propre PC (adresse `127.0.0.1`, port fixe `47829`) pour recevoir la réponse, puis échange un code contre des jetons.
- **Où sont les jetons** : dans le **gestionnaire d'identifiants Windows** (identifiants génériques nommés `Oniris/…`, conservés sur cette machine seulement). Ils ne sont **jamais** écrits dans `oniris.db`, dans un fichier, ni dans les journaux. La base ne garde que l'adresse du compte, sa couleur et son état (« ok » ou « à reconnecter »).
- **Autorisations demandées** : pour Google, ton adresse mail, Gmail en lecture et modification (lire, archiver, mettre à la corbeille, marquer comme lu, envoyer) et Agenda en lecture seule ; pour Spotify, l'état de la lecture, son pilotage et tes playlists (en lecture seule).
- **Si l'accès est retiré** : Oniris affiche « À reconnecter » ; il ne réessaie pas en boucle.
- **Supprimer un compte** dans la feuille « Comptes » efface ses jetons du coffre et ses données en cache (mails et événements).

### Tout révoquer

Pour couper l'accès d'Oniris à tes comptes, depuis le site du fournisseur (cela marche même si Oniris n'est plus installé) :

- Google : [myaccount.google.com/permissions](https://myaccount.google.com/permissions), puis ton application, puis « Supprimer l'accès » ;
- Spotify : [spotify.com/account/apps](https://www.spotify.com/account/apps), puis « Retirer l'accès » en face de ton application.

Tu peux aussi supprimer les identifiants `Oniris/…` dans le gestionnaire d'identifiants Windows.

## Mails (Gmail)

Les échanges se font directement entre ton PC et Google, par l'API officielle de Gmail. Rien ne passe par un serveur d'Oniris (il n'y en a pas).

- **Relève** : environ toutes les 3 minutes (réglable de 2 à 5), même panneau fermé, sur la boîte de réception seulement, de façon incrémentale.
- **Ce qui est gardé en local** (`oniris.db`) : pour chaque mail de la boîte de réception, l'expéditeur (nom et adresse), le **sujet**, l'**extrait** fourni par Gmail (les premiers mots), la date, les libellés, l'état lu / non lu et l'onglet (« À traiter », « À lire » ou « Filtrés »). Retirer un compte efface ces lignes.
- **Ce qui n'est jamais gardé** : le **corps** des mails et les pièces jointes. Un mail n'est téléchargé en entier que quand tu l'ouvres ; il reste alors **en mémoire seulement** (les 20 derniers ouverts) et disparaît à la fermeture d'Oniris. Il n'est écrit ni dans la base, ni dans un fichier, ni dans les journaux.
- **Lecture** : le mail s'affiche dans un cadre isolé, sans script. Les **images distantes sont bloquées** tant que tu ne cliques pas sur « Afficher les images » (pour ce mail seulement) : sinon, elles préviendraient l'expéditeur que tu as ouvert son message. Les liens ne s'ouvrent que quand tu les choisis.
- **Actions** : archiver, mettre à la corbeille (avec confirmation) et marquer comme lu modifient ton compte Gmail. Un mail n'est **envoyé qu'après une carte de confirmation** que tu valides ; il part de ton propre compte vers le destinataire que tu as écrit.
- **Journal** : le sujet et le texte d'un mail envoyé n'y figurent jamais, seulement leur taille.

## Agenda Google

Si tu as autorisé l'agenda, Oniris lit tes agendas Google **en lecture seule** (il ne crée, ne modifie et ne supprime rien chez Google), à chaque relève des mails.

- Gardés en local, dans `oniris.db` : le **titre**, les horaires, le lieu éventuel et l'agenda d'origine de chaque événement proche. Les invités, les descriptions et les pièces jointes ne sont pas gardés.
- Ils s'affichent dans la frise et dans la carte Agenda. Retirer le compte efface ces lignes.
- Les titres d'événements Google ne sont pas gardés dans le journal des actions (seulement le nombre d'événements lus).

## Musique : lecture Windows et Spotify

- **Lecture Windows (SMTC)** : pour afficher le titre, l'artiste et la pochette, Oniris lit ce que Windows sait des médias en cours (Spotify, navigateur, lecteur de musique), **uniquement sur ton PC et seulement quand le panneau est ouvert**. Rien n'est envoyé sur Internet pour cela. Le titre, l'artiste et la pochette restent en mémoire : ils ne sont écrits ni sur le disque ni dans les journaux. Les couleurs de la lumière ambiante sont calculées en local à partir de la pochette.
- **Spotify (Web API)** : si tu connectes ton compte Spotify, Oniris envoie à Spotify les commandes que tu donnes (lancer une playlist, changer d'appareil, volume, saut dans le morceau) et lui demande la liste de tes appareils et de tes playlists, **seulement quand le panneau est ouvert**. Les playlists restent en mémoire une dizaine de minutes ; la base ne garde que les identifiants des playlists que tu as choisies comme favorites. Spotify voit, comme tout service, l'adresse IP de ta connexion. Aucune requête Spotify n'est faite panneau fermé.

## Sondes des sites

Si tu ajoutes des sites dans la carte « VPS et favoris », Oniris envoie **chaque minute** une requête `GET` simple à chacun de ces sites, et seulement à eux, pour savoir s'ils répondent et en combien de temps :

- aucun cookie n'est envoyé ni conservé, et rien d'autre qu'une requête ordinaire (l'agent annoncé est `Oniris-sonde/<version>`) ; Oniris lit au plus 64 Ko de la réponse, puis l'abandonne sans la garder ;
- la base garde, pour chaque site, la date, le code de réponse et le temps de réponse, pendant **24 heures** au plus ; la purge est faite à chaque relève ;
- en cas de panne ou de lenteur, Oniris t'envoie une notification Windows locale avec le nom que tu as donné au site ; rien n'est envoyé à un tiers ;
- les journaux techniques ne contiennent ni adresse ni nom de site ;
- sans site dans la liste, Oniris n'envoie aucune requête de ce genre.

## Charge du VPS (Zenytt)

Si Zenytt est installé sur ce PC, Oniris lance son serveur en lecture seule (`Zenytt --mcp`) sur ton PC, quand le panneau est ouvert, et l'arrête 10 minutes après sa fermeture :

- c'est Zenytt qui se connecte au VPS avec ses propres réglages ; Oniris ne voit, ne stocke ni ne gère **aucun** identifiant SSH ;
- seules des mesures agrégées sont lues et affichées : CPU, mémoire, disque, charge et nombre d'alertes, toutes les 30 secondes panneau ouvert, jamais en jeu plein écran ; rien n'est écrit en base ;
- « Importer depuis Zenytt » lit la liste des domaines HTTPS de tes sites nginx et ajoute ceux qui manquent aux sites surveillés ;
- les journaux techniques ne contiennent que le genre d'une erreur, jamais de nom de serveur ni d'hôte ;
- sans Zenytt, rien de tout cela ne se produit.

## IRIS : le cerveau local

IRIS comprend d'abord tes demandes avec un modèle de langage qui tourne **sur ton PC**, via [Ollama](https://ollama.com) (par défaut `qwen3:14b`). Oniris ne parle à Ollama que sur ton propre PC (`127.0.0.1`) : ce que tu dis à IRIS et ce que le modèle lit ou répond ne sort pas de ta machine. Certaines demandes simples (« suivant », « rappelle-moi de… ») sont même comprises sans modèle. Le modèle se charge à ta première demande et se décharge après quelques minutes d'inactivité, et jamais pendant un jeu ou une app en plein écran.

Le tri de tes mails, le briefing du matin et les brouillons « en local » utilisent aussi ce cerveau. Le résultat du tri (une étiquette et un court résumé par mail) est gardé dans `oniris.db` avec l'en-tête du mail ; il disparaît avec le compte.

## IRIS : le téléchargement du modèle

Oniris n'installe pas Ollama et ne télécharge rien tout seul. Si le modèle manque, un bouton « Télécharger le modèle » dans les réglages d'IRIS demande à ton Ollama local de le récupérer : **c'est Ollama qui contacte ses propres serveurs**, et seulement après ton clic. Ils voient, comme tout site web, l'adresse IP de ta connexion et le nom du modèle demandé.

## IRIS : Claude

Pour rédiger (« Répondre avec IRIS »), raisonner ou répondre à ce qui dépasse le cerveau local, IRIS peut faire appel à Claude. Oniris lance alors **Claude Code sur ton PC**, avec **ton propre abonnement** (aucune clé n'est stockée par Oniris) ; c'est Claude Code qui communique avec Anthropic, comme quand tu le lances toi-même. **Par défaut, Claude n'est utilisé que si tu le demandes** : le bouton Claude, « demande à Claude… », ou « Répondre avec IRIS ». Si une demande dépasse le cerveau local, ou si Ollama n'est pas disponible, IRIS te **propose** un bouton « Demander à Claude » ; rien ne part sans ton clic. Dans la feuille IRIS, la case « IRIS peut appeler Claude d'elle-même » rétablit l'ancien fonctionnement : IRIS confie alors elle-même à Claude ce qui la dépasse (et te le signale si Ollama est indisponible).

Ce qui part chez Anthropic dans ce cas :

- ta demande ;
- les **six derniers échanges** de la conversation ;
- tes **préférences retenues** (voir « Conversations et mémoire ») et la personnalité que tu as décrite ;
- les **résultats des outils** utilisés pendant le tour, dans une version réduite, **sans le contenu venant de l'extérieur** (corps de mails, pages, fichiers lus) : Claude n'en voit que des références du genre `{{u1}}`, pas le texte ;
- le **contenu que tu demandes de résumer en détail ou auquel tu demandes de répondre**, par le bouton « Répondre avec IRIS » ou dans la conversation : un mail (expéditeur, sujet, texte, tronqué à 8 Ko pour le bouton), mais aussi un élément du presse-papiers, un événement d'agenda externe, etc. Jamais pour le tri ni pour le briefing, qui restent locaux.

Claude ne reçoit aucun outil propre : il n'agit que par les outils d'Oniris, avec les mêmes cartes de confirmation que si c'était IRIS en local. Pour cela, Oniris ouvre un petit canal **sur ton PC seulement**, protégé par un jeton aléatoire valable le temps d'un tour. Un mail qui contiendrait des consignes ne peut déclencher aucune action sans qu'une carte te la montre d'abord.

## Conversations et mémoire

- Tes conversations avec IRIS (tes messages, ses réponses) sont gardées dans `oniris.db`. Tu peux les **consulter et les effacer** depuis les réglages d'IRIS ; effacer l'historique ne supprime pas tes préférences.
- Les **préférences** (« retiens que ma playlist de sport, c'est X ») ne sont enregistrées que si tu les demandes ; elles sont visibles, modifiables et supprimables dans les réglages. IRIS ne tire aucun profil de ton usage.
- Les extraits de mails qu'IRIS a lus pendant un tour ne sont pas gardés dans la conversation : seules des références y figurent.

## IRIS : la voix

La voix est facultative et se passe entièrement **sur ton PC**.

- **Micro** : il n'est ouvert que pendant que tu maintiens Alt+Espace, et se referme au relâchement (au plus 30 secondes d'affilée). Oniris n'écoute jamais en continu et n'a pas de mot d'activation. Le son reste **en mémoire** : il n'est écrit ni sur le disque, ni dans la base, ni dans les journaux, et n'est envoyé nulle part.
- **Transcription** : [Whisper](https://github.com/ggml-org/whisper.cpp) tourne sur ton processeur, en local (`127.0.0.1`). Le programme se lance au premier usage et s'arrête après quelques minutes d'inactivité ; il s'arrête aussi dès l'entrée en plein écran (jeu, vidéo), et juste après chaque transcription faite en plein écran.
- **Synthèse** : [Piper](https://github.com/rhasspy/piper) lit les réponses d'IRIS à voix haute, en local, avec une voix française. Tu peux le désactiver (réponses écrites seulement) et régler le volume. Les textes lus ne sont gardés nulle part et n'apparaissent pas dans les journaux.
- **Ce que tu dis à IRIS** est transcrit en texte, puis suit **exactement le même chemin qu'une demande écrite** (voir « IRIS : le cerveau local » et « IRIS : Claude ») : local par défaut, et Claude seulement dans les cas décrits plus haut. Le texte transcrit s'ajoute à la conversation, dans `oniris.db`, comme un message écrit.
- **Pause** : « Mettre la voix en pause » dans le menu de l'icône d'Oniris coupe l'écoute.

## IRIS : le téléchargement de la voix

Oniris ne télécharge la voix que **sur ton clic** (« Installer la voix » dans les réglages d'IRIS) :

- les programmes viennent de **GitHub** (Whisper, Piper) ; les modèles de Whisper et la voix de Piper viennent de **Hugging Face** ;
- chaque fichier est vérifié (taille et empreinte SHA-256) avant d'être gardé ;
- GitHub et Hugging Face voient, comme tout site web, l'adresse IP de ta connexion et le nom des fichiers demandés ; Oniris n'envoie rien d'autre, et ne les recontacte pas ensuite (sauf si tu passes au modèle « précis », qui demande un fichier de plus) ;
- tout est rangé dans `%LOCALAPPDATA%\Oniris\voice`.

## IRIS : le mot d'activation

Le mot d'activation (« Dis Iris » ou « Hé Iris ») est **désactivé par défaut**. Tu l'actives toi-même dans la section « Voix » de la feuille IRIS.

- **Ce qui est écouté** : une fois activé, Oniris écoute le micro en continu, sur ton PC, pour repérer ce seul mot. Windows affiche l'icône du micro tant que l'écoute est active.
- **Les 2 dernières secondes, en mémoire** : avant la détection, l'audio ne vit que dans la mémoire du détecteur, sur 2 secondes au plus, et s'efface au fur et à mesure.
- **Aucune écriture** : rien n'est écrit sur le disque, transcrit, journalisé ni envoyé avant la détection. Après la détection, l'audio suit les mêmes règles que le raccourci Alt+Espace : 30 secondes au plus, en mémoire, vers Whisper sur ton PC seulement.
- **Suspensions** : l'écoute s'arrête quand la voix est en pause (zone de notification), quand une application est en plein écran, pendant qu'IRIS parle et un court instant après, et pendant que tu utilises le raccourci.
- **Le modèle** : il est fourni avec l'app et tourne sur ton PC. Il a été entraîné sur des voix synthétiques et, si tu l'as fait, sur tes propres enregistrements, qui restent sur ton PC.
- Désactiver le réglage arrête l'écoute immédiatement.

## Journal des commandes et des fichiers

IRIS peut exécuter une commande PowerShell, gérer des fichiers et lancer, fermer ou mettre au premier plan une application, toujours dans des limites fixes (pas d'administrateur, dossiers système et `%APPDATA%\Oniris` protégés, suppressions envoyées à la **Corbeille** et jamais définitives). Le journal des actions garde pour cela la **commande exacte**, les **chemins complets** des fichiers concernés et les noms d'applications, pour que tu puisses voir ce qui s'est passé et annuler quand c'est possible. Le résultat, lui, est raccourci. Les journaux techniques, eux, ne contiennent ni commande ni chemin. Le journal d'actions n'est pas encore purgé automatiquement.

## Démarrage avec Windows et désinstallation

- **Démarrage automatique** : Oniris ne s'ajoute au démarrage de Windows (clé `Run` de ton compte, valeur « Oniris ») qu'**après ton accord**, demandé une seule fois au premier lancement. Tant que tu n'as pas fini l'accueil ni répondu, rien n'est écrit. Tu peux le changer ensuite dans les réglages.
- **Journaux techniques** : ils sont dans `%APPDATA%\Oniris\logs` et ne contiennent aucun contenu venant de toi (voir « Où sont tes données »).
- **Installeur** : il s'installe pour ton compte seulement (pas d'administrateur), en français. À la désinstallation, il retire la valeur « Oniris » de la clé `Run` mais **garde volontairement** `%APPDATA%\Oniris` et `%LOCALAPPDATA%\Oniris` (tes données et la voix), pour qu'une réinstallation retrouve tout. Pour tout effacer, supprime ces deux dossiers toi-même, ainsi que `%LOCALAPPDATA%\dev.oniris.app` (cache WebView2 : images et icônes récupérées en https).

## À venir

Ces fonctions n'existent pas encore. Chacune ajoutera sa propre section ici avant d'être disponible. Outlook n'est pas prévu : seul Gmail est géré.

- Le mode apprentissage du mot d'activation (enregistrer des extraits pour améliorer le modèle) : pas encore disponible ; il aura sa propre section ici avant d'exister.
