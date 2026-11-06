# Plan 4 : vérifications à faire à la main

Les modules du plan 4 (comptes Google et Spotify, mails, agenda, musique, sites, Zenytt) parlent à de vrais services et à Windows : OAuth, API Gmail et Agenda, lecture des médias, coffre d'identifiants, notifications. Aucun test n'a touché à Internet, à ton compte Google, à Spotify, à Zenytt ni à ton bureau réel : tout a été vérifié avec de faux serveurs sur ton PC. Cette liste est donc la première vraie rencontre avec tes comptes. Compte environ 1 h 30, plus le temps de créer tes applications Google et Spotify.

Les cases marquées « ⚠️ à vérifier en priorité » sont les plus risquées.

Pour ne rien envoyer à de vraies personnes pendant les essais, tous les mails de test partent **vers toi-même**.

## Lancer l'app en mode développement

Dans `Projets/Oniris` :

```powershell
pnpm tauri dev
```

Ouvre le panneau avec Alt+Espace. Pour voir les logs, ouvre le dossier `%APPDATA%\Oniris\logs` (fichier `oniris.log.<date>`).

Les chiffres de performance ne se mesurent pas ici : ils viennent du build de mesure (section 15 du plan 3 et [`tools/perf/README.md`](../../tools/perf/README.md)), pas de `pnpm tauri dev`.

Pendant ces essais, garde sous la main : ton compte Google (Gmail et Agenda), ton compte Spotify Premium, Spotify ouvert sur le PC, un navigateur qui peut lire une vidéo, et Zenytt installé.

## 1. Coffre Windows et premier lancement

- [ ] ⚠️ à vérifier en priorité : le vrai coffre. Dans `Projets/Oniris`, lance :

  ```powershell
  cargo test -p oniris secrets_win -- --ignored
  ```

  Le test écrit, relit, remplace puis supprime un identifiant de test dans le gestionnaire d'identifiants Windows, et doit passer. Il ne laisse aucun identifiant `Oniris/test-…` derrière lui (vérifie dans « Gestionnaire d'identifiants », section « Informations d'identification Windows »).
- [ ] Au tout premier lancement, la feuille « Comptes » (bouton « Comptes » en haut à gauche du panneau) s'ouvre avec les guides de configuration visibles, sans compte connecté.
- [ ] Les cartes « En lecture » et « Mails » invitent à connecter un compte au lieu d'afficher « Bientôt ».

## 2. Configuration Google et Spotify, en suivant le guide

- [ ] Suis le guide Google pas à pas : projet Google Cloud, écran de consentement, application passée **« En production »** (pas « Test »), activation des API Gmail et Agenda, identifiant client de type « Application de bureau ». Chaque étape du guide correspond à ce que tu vois dans la console, sans terme obsolète ni bouton introuvable. Note toute étape qui ne colle plus.
- [ ] Les boutons « Ouvrir la console » ouvrent le bon site dans ton navigateur, et les boutons « Copier » copient bien la valeur à coller.
- [ ] Colle l'identifiant client et le secret client Google : l'enregistrement réussit. Un secret avec des espaces ou trop long est refusé avec un message clair.
- [ ] Suis le guide Spotify : application dans Spotify for Developers, URI de redirection exacte, ton compte ajouté comme utilisateur autorisé (mode développement), identifiant client collé.
- [ ] Les valeurs à copier (adresse de redirection) sont exactement celles que Google et Spotify acceptent. Pas d'erreur « redirect_uri » au moment de te connecter.
- [ ] Le panneau se ferme quand le navigateur prend le focus, c'est connu. Rouvre-le : la feuille « Comptes » est toujours à l'étape de connexion, la connexion en cours n'est pas perdue. Note si ce comportement te gêne.

## 3. Connexion Google (deux comptes Gmail)

- [ ] ⚠️ à vérifier en priorité : OAuth Google en production. « Connecter Google » ouvre ton navigateur sur la page de Google ; l'application est en production **sans vérification**.
- [ ] L'écran « Google n'a pas validé cette application » apparaît. Tu peux continuer (« Paramètres avancés », puis « Accéder à … ») et accepter les autorisations (Gmail et Agenda). Le guide t'avait prévenu de cet écran.
- [ ] Après acceptation, la page du navigateur dit que c'est fini, et la feuille « Comptes » affiche l'adresse Gmail avec « Connecté » et une couleur.
- [ ] Connecte un **deuxième compte Gmail** : les deux apparaissent, avec deux couleurs différentes.
- [ ] Reconnecter un compte déjà connecté ne crée pas de doublon.
- [ ] Ferme la page de connexion sans accepter : Oniris ne reste pas bloqué sur « Connexion en cours », tu peux réessayer.
- [ ] Les jetons ne tombent pas au bout d'une semaine : l'application est bien en production. Vérifie à nouveau 7 jours plus tard que les comptes sont toujours « Connecté ».
- [ ] Une notification Windows confirme la connexion (si le panneau s'est fermé entre-temps).

## 4. Mails : relève et onglets

- [ ] La carte « Mails » affiche tes mails de la boîte de réception des deux comptes, avec une pastille de la couleur de chaque compte sur l'avatar de l'expéditeur.
- [ ] Les trois onglets « À traiter », « À lire » et « Filtrés » affichent des compteurs et classent tes mails de façon plausible (le tri fin par IRIS arrive plus tard : ici, c'est un tri simple par règles).
- [ ] Envoie-toi un mail depuis un autre moyen : il apparaît dans la carte en moins de 5 minutes, panneau ouvert **et** panneau fermé (ouvre-le ensuite pour voir).
- [ ] Marque un mail comme lu ou non lu depuis Gmail : l'état change dans Oniris à la relève suivante.
- [ ] Un mail supprimé ou archivé dans Gmail disparaît de la carte à la relève suivante.
- [ ] Le premier chargement d'un gros compte (des milliers de mails) ne fige pas le panneau et ne dépasse pas les limites de Google (pas d'erreur dans la feuille « Comptes »).
- [ ] Coupe le Wi-Fi quelques minutes : la carte affiche « hors ligne · il y a N min » avec le dernier contenu connu, et aucune notification en rafale. Au retour du réseau, la relève reprend toute seule.

## 5. Mails : lecture, images bloquées, liens

- [ ] Clique sur un mail : il s'ouvre dans le panneau (cadre isolé).
- [ ] Ouvre une newsletter en HTML avec des images : les images distantes **ne se chargent pas** tant que tu ne cliques pas sur « Afficher les images ». Après le clic, elles s'affichent, pour ce mail seulement : un autre mail les bloque de nouveau.
- [ ] Aucun script ne s'exécute, et un mail piégé (formulaire, iframe, balise `<style>` qui déborde) ne casse pas la mise en page du panneau.
- [ ] « Liens du mail » liste les liens ; un clic ouvre le lien dans ton navigateur (et non dans le panneau). Un lien `javascript:` ou un lien vers un fichier n'est pas ouvert.
- [ ] Ouvrir un mail non lu le marque comme lu dans Gmail.
- [ ] « Ouvrir dans Gmail » ouvre le bon fil, sur le bon compte (vérifie avec deux comptes).
- [ ] Un mail très long, avec des pièces jointes ou une conversation en cascade, s'affiche sans bloquer le panneau, avec défilement dans le lecteur.

## 6. Mails : archiver, annuler, supprimer

- [ ] Archiver un mail depuis la carte : il disparaît de la liste, et dans Gmail il n'est plus dans la boîte de réception mais reste dans « Tous les messages ».
- [ ] L'annulation de l'archivage est prévue par le journal des actions (un bouton « Annuler » dans la carte n'existe pas encore). Remets le mail dans la boîte de réception depuis Gmail : il réapparaît dans Oniris à la relève suivante.
- [ ] Supprimer un mail demande d'abord une **carte de confirmation** montrant l'expéditeur et le sujet. « Refuser » ne supprime rien. « Valider » envoie le mail dans la corbeille de Gmail (récupérable pendant 30 jours), pas en suppression définitive.
- [ ] Aucune action sur un mail ne se fait sans toi : aucun mail n'est archivé, supprimé ou envoyé tout seul pendant les relèves.

## 7. Mails : répondre (à toi-même uniquement)

- [ ] ⚠️ à vérifier en priorité : la réponse envoyée **à soi-même**. Envoie-toi d'abord un mail de test depuis une autre adresse à toi. Ouvre-le dans Oniris, clique sur « Répondre », mets **ta propre adresse** dans « À », écris un texte avec des accents (é, è, à, ç, œ) et des sauts de ligne.
- [ ] « Envoyer » n'envoie rien tout de suite : une carte de confirmation apparaît, avec le compte, le destinataire, le sujet et le **texte entier**, sans troncature.
- [ ] « Refuser » : rien n'est envoyé. « Valider » : le mail part.
- [ ] Le mail arrive bien dans ta boîte, **dans le même fil** que le mail d'origine (sujet « Re: … », pas de « Re: Re: »), avec les accents et les sauts de ligne intacts.
- [ ] Attends plus de quelques minutes avant de valider une carte : elle indique « Action expirée, recommence. » et rien ne part.
- [ ] Après l'envoi, ouvre `%APPDATA%\Oniris` et vérifie dans les logs qu'on n'y trouve ni le sujet ni le texte du mail.
- [ ] N'essaie **pas** de répondre à une vraie personne pendant ces essais.

## 8. Transfert Orange

- [ ] Suis le guide « adresse Orange » : active le transfert côté Orange vers ton Gmail.
- [ ] Un mail envoyé à ton adresse Orange arrive dans Gmail, puis dans la carte « Mails » d'Oniris. Aucun code spécifique à Orange n'est nécessaire.

## 9. Agenda Google dans la frise

- [ ] Crée un événement dans Google Agenda pour aujourd'hui : il apparaît dans la frise et dans la liste du jour de la carte Agenda, avec la couleur de l'agenda Google.
- [ ] Un événement sur la journée entière s'affiche comme tel. Un événement d'un autre agenda du même compte (agenda partagé coché) apparaît aussi.
- [ ] Modifie ou supprime l'événement dans Google : il se met à jour dans Oniris à la relève suivante.
- [ ] Un clic sur un événement Google ne propose pas de le supprimer (lecture seule). Les événements créés dans Oniris, eux, restent supprimables. Note un cas particulier connu : demander la suppression d'un événement Google (par IRIS plus tard) affichera une carte de confirmation vouée à l'échec.
- [ ] Les points du mini-calendrier couvrent aussi les jours avec des événements Google.
- [ ] Supprime un compte dans « Comptes » : ses événements disparaissent de la frise.

## 10. Musique : lecture Windows (SMTC)

- [ ] ⚠️ à vérifier en priorité : SMTC réel. Avec **Spotify** qui joue, ouvre le panneau : le titre, l'artiste et la pochette apparaissent dans « En lecture », avec trois couleurs de lumière ambiante.
- [ ] Passe à un morceau d'un autre artiste, puis à un autre morceau du même artiste : la pochette est relue quand l'artiste change (elle peut mettre un instant), et ne reste pas celle du morceau précédent.
- [ ] Change de morceau dans Spotify : le titre et la pochette se mettent à jour en moins d'une seconde. Pendant des changements rapides, aucune ancienne pochette ne reste affichée avec le nouveau titre.
- [ ] Mets en pause : l'égaliseur se fige et l'icône passe sur « lecture ».
- [ ] ⚠️ à vérifier en priorité : avec une **vidéo de navigateur** (Chrome ou Edge) **et** Spotify qui jouent en même temps, c'est la session Spotify qui est suivie.
- [ ] Arrête Spotify et ne laisse que la vidéo du navigateur : sa musique ou son titre s'affiche, avec la mention de la source « autre ».
- [ ] Les boutons précédent, lecture/pause et suivant fonctionnent sur Spotify et sur le navigateur.
- [ ] La barre de progression : un clic ou un glissement déplace la lecture sur Spotify. Sur un lecteur qui ne le permet pas, un message français s'affiche au lieu d'un plantage.
- [ ] Le temps écoulé reste juste dans la durée (pas de dérive visible après 1 ou 2 minutes, ni après une pause).
- [ ] Ferme le panneau : plus aucune lecture SMTC (le CPU reste à environ 0 %). Rouvre-le : l'état est à jour tout de suite.
- [ ] Démarre Spotify **pendant** que le panneau est ouvert : le titre apparaît sans rouvrir le panneau.
- [ ] Les logs ne contiennent ni titre ni artiste.

## 11. Musique : Spotify (connexion et playlists)

- [ ] ⚠️ à vérifier en priorité : « Connecter Spotify » ouvre la page Spotify. Accepte : la feuille « Comptes » affiche ton compte Spotify « Connecté ». Un compte non Premium, ou non déclaré comme utilisateur de ton application, doit donner un message clair.
- [ ] Les playlists favorites s'affichent en vignettes sous les commandes. Tes playlists sont lues sans erreur.
- [ ] Clique sur une playlist : la lecture démarre sur l'appareil choisi. Juste après un changement d'appareil, la lecture démarre immédiatement (comportement non vérifié pendant le développement, note si elle tarde).
- [ ] Le menu « … » permet de choisir tes playlists favorites (8 au plus) ; le choix est gardé après un redémarrage.
- [ ] Le sélecteur d'appareil liste « Ce PC » et tes autres appareils Spotify ; changer d'appareil transfère la lecture.
- [ ] Sans playlist ni connexion Spotify, la carte reste utile avec la lecture Windows seule, sans erreur.

## 12. Musique : Spotify fermé

- [ ] Ferme complètement l'application Spotify. La carte « En lecture » ne plante pas : elle affiche un état vide ou un message simple.
- [ ] Clique sur une playlist avec Spotify fermé : un message clair explique qu'aucun appareil n'est disponible (ou propose d'ouvrir Spotify), au lieu d'une erreur technique.
- [ ] Rouvre Spotify : la carte reprend sans relancer Oniris.
- [ ] Panneau fermé, **aucune** requête Spotify ne part : vérifie le trafic ou les logs de `oniris.log.<date>` (aucune ligne Spotify entre la fermeture et la réouverture).

## 13. Lumière ambiante

- [ ] La lumière de fond du panneau prend les couleurs de la pochette en cours, avec un fondu doux (environ une seconde) et sans clignoter à chaque changement de titre.
- [ ] Une pochette très sombre ou très claire donne une ambiance lisible, sans couleur criarde.
- [ ] Avec les animations réduites dans Windows, l'égaliseur reste statique et le fond ne bouge plus.
- [ ] Sans musique, l'ambiance revient aux halos d'origine.

## 14. Sites en panne et PC hors ligne

- [ ] Dans la carte « VPS et favoris », ajoute un de tes vrais sites : sa pastille devient verte avec un temps de réponse et une petite courbe.
- [ ] Panne simulée : ajoute un site à une adresse invalide (par exemple `https://ce-site-nexiste-pas.example`). Après deux tours de sondes (environ 2 minutes), sa pastille devient rouge, et **une** notification Windows annonce la panne. Pas de notification répétée à chaque minute.
- [ ] Retire l'adresse invalide ou corrige-la : la pastille redevient verte, et une notification annonce le rétablissement.
- [ ] La liste des sites reste lisible et triée par gravité (les sites en panne en premier). Avec beaucoup de sites, elle défile sans déformer la carte.
- [ ] ⚠️ à vérifier en priorité : PC hors ligne sans rafale. Coupe le Wi-Fi ou débranche le câble pendant 3 à 4 minutes, avec plusieurs sites listés : **aucune** notification « site en panne » ne part, et l'historique n'enregistre pas de panne. Rebranche : les sites repassent au vert sans notification.
- [ ] Si tu peux le simuler, une vraie panne du VPS (par exemple en bloquant son adresse dans le pare-feu, tout en gardant Internet) fait bien passer tous les sites au rouge et notifie : ce n'est pas pris pour une absence de réseau.
- [ ] Sans aucun site listé, Oniris n'envoie aucune sonde.
- [ ] Les logs ne contiennent aucune adresse de site.

## 15. Zenytt : charge du VPS et import des sites

- [ ] ⚠️ à vérifier en priorité : Zenytt réel. Zenytt est installé dans `%LOCALAPPDATA%\Zenytt` (l'exécutable s'appelle `zenytt-desktop.exe`). Ouvre le panneau : Oniris détecte Zenytt tout seul et la carte « VPS et favoris » affiche une ligne avec le nom du serveur, le CPU, la RAM et le disque, sans console qui clignote.
- [ ] Les chiffres correspondent à ce que montre Zenytt lui-même au même moment. Ils se rafraîchissent toutes les 30 secondes environ.
- [ ] Première connexion à froid : la première mesure peut prendre jusqu'à 25 secondes (connexion SSH). Pendant ce temps, la carte reste utilisable et affiche un état d'attente, pas une erreur.
- [ ] Cas d'erreur de Zenytt : clé SSH jamais approuvée, ou aucun serveur ouvert à l'IA dans Zenytt. La ligne VPS affiche un message court et compréhensible (sans nom de serveur ni détail technique brut), pas un plantage, et le processus Zenytt reste vivant.
- [ ] « Importer depuis Zenytt » ajoute les domaines HTTPS de tes sites nginx aux sites surveillés ; un second import dit « aucun nouveau » et ne crée pas de doublon.
- [ ] Sans Zenytt (renomme son dossier le temps d'un essai, puis remets-le), la ligne VPS disparaît et le bouton d'import aussi. Le reste de la carte marche.
- [ ] Après la fermeture du panneau, Zenytt (`--mcp`) est arrêté au bout d'environ 10 minutes : vérifie dans le Gestionnaire des tâches qu'il n'en reste pas.
- [ ] Quitte Oniris par le menu de la zone de notification (« Quitter ») : aucun processus Zenytt ne reste dans le Gestionnaire des tâches.
- [ ] Oniris ne te demande, ne stocke et ne gère aucun identifiant SSH.
- [ ] Zenytt ne se lance pas en jeu plein écran avec le panneau ouvert (pas de mesure toutes les 30 s).

## 16. Révocation d'un accès et « À reconnecter »

- [ ] ⚠️ à vérifier en priorité : sur [myaccount.google.com/permissions](https://myaccount.google.com/permissions), retire l'accès d'Oniris à un des deux comptes Gmail. À la relève suivante (au plus quelques minutes), la carte « Mails » affiche le bandeau « Reconnecte … pour relever ses mails » (état « À reconnecter ») pour ce compte, sans casser l'autre compte, et sans tempête de notifications ni de requêtes en boucle (vérifie le log).
- [ ] La feuille « Comptes » affiche « À reconnecter » avec un bouton « Reconnecter » ; il relance la connexion et le compte repasse à « Connecté », avec ses mails revenus.
- [ ] Retire de la même façon l'accès de Spotify sur [spotify.com/account/apps](https://www.spotify.com/account/apps) : « En lecture » repasse sur la lecture Windows seule et la feuille « Comptes » propose de reconnecter Spotify.
- [ ] L'icône corbeille (« Supprimer ») d'un compte dans la feuille « Comptes » supprime ses mails, ses événements et ses jetons : plus de ligne dans la carte Mails ni dans la frise, et l'identifiant `Oniris/…` du compte a disparu du gestionnaire d'identifiants Windows.

## 17. Panneau et mise en page

- [ ] Sur ton 2560×1440, le panneau tient en entier, **sans estompage** en bas de la grille : Notes, Presse-papiers et Minuteur sont visibles en entier, et rien ne se chevauche. Les cartes « En lecture », « Mails » et « Agenda » ne paraissent pas tassées.
- [ ] Avec les vraies données (beaucoup de mails, des événements, des rappels), la carte Agenda et la carte Mails ne dépassent pas leur emplacement : les listes défilent à l'intérieur.
- [ ] Sur un écran plus petit (1080p), la grille défile à la molette avec un fondu en bas, la barre de commande reste visible et aucune carte n'en recouvre une autre.
- [ ] Si ton affichage Windows est mis à l'échelle (125 % par exemple), le panneau reste utilisable : la grille défile au besoin.
- [ ] Ouvre et ferme le panneau 5 fois de suite : aucune fenêtre de console ne clignote (Zenytt, `git`).

## 18. Logs et vie privée

- [ ] Les logs (`%APPDATA%\Oniris\logs`) ne contiennent aucune adresse mail, aucun sujet, aucun titre d'événement, aucun titre de morceau, aucun nom de site ni aucun jeton : seulement des compteurs, des identifiants et des codes de réponse. Fais une recherche des mots « Bearer », « access_token », « refresh_token » et de ton adresse Gmail : aucun résultat.
- [ ] Aucun jeton n'est visible dans `%APPDATA%\Oniris\oniris.db` (ouvre-la avec un lecteur SQLite si tu en as un : la table `accounts` n'a que l'adresse, la couleur et l'état).
- [ ] La table `mails` ne contient que des en-têtes et un extrait : aucun corps de mail.
- [ ] Le journal des actions ne contient ni le sujet ni le texte d'un mail envoyé, ni le titre des événements Google lus.
- [ ] Le document de confidentialité (`docs/legal/confidentialite.md`, version 4) correspond à ce que tu observes : comptes et jetons, mails, agenda, musique, sondes, Zenytt, et la procédure de révocation.

## Si quelque chose ne va pas

Note la section et le début de la case (par exemple « section 7, réponse à toi-même ») et ce que tu as vu, puis envoie-le-moi. Les points les plus incertains sont le vrai coffre Windows (section 1), la connexion Google en production (section 3), la réponse à toi-même (section 7), la lecture Windows réelle (section 10), la règle hors ligne (section 14), Zenytt réel (section 15) et la révocation d'un accès (section 16).
