# Confidentialité d'Oniris

Version 2 — 4 novembre 2026. Ce document décrit simplement ce qu'Oniris fait de tes données. Il est mis à jour à chaque nouveauté qui touche à ta vie privée.

## En bref

Oniris range tout chez toi, sur ton PC. Il n'envoie aucune statistique d'usage, et il n'y a aucun compte à créer pour l'utiliser. La seule communication avec Internet, aujourd'hui, est la météo, et seulement si tu as choisi une ville.

## Où sont tes données

Tout est stocké en local, dans le dossier `%APPDATA%\Oniris` :

- la base de données `oniris.db` : réglages, agenda, rappels, notes, minuteurs, historique du presse-papiers, journal des actions ;
- les journaux techniques (`logs`) : ils ne contiennent ni le texte de tes notes, ni celui de tes rappels, ni celui de ton presse-papiers. Seulement des identifiants, des tailles et des messages d'erreur.

Supprimer ce dossier efface tout ce qu'Oniris sait de toi.

## Ce que garde la base

- **Suppression d'une note, d'un rappel ou d'un événement** : pour l'instant, Oniris ne fait que le masquer. Le texte reste dans `oniris.db` jusqu'à une purge, prévue dans une version ultérieure. Seul le fait de supprimer le dossier `%APPDATA%\Oniris` l'efface aujourd'hui à coup sûr.
- **Journal des actions** : chaque action (créer une note, cocher un rappel, lancer un minuteur…) y est enregistrée avec ses arguments, y compris le texte d'une note, d'un rappel ou d'un événement. Ce journal n'est pas encore purgé automatiquement.
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

## Historique du presse-papiers

L'historique reste sur ton PC ; il n'est envoyé nulle part. Ses limites :

- les copies faites depuis un gestionnaire de mots de passe qui les signale comme sensibles sont exclues ;
- en revanche, les **extensions de gestionnaires de mots de passe dans le navigateur ne signalent pas leurs copies** : un mot de passe copié depuis une telle extension peut donc être enregistré dans l'historique ;
- tu peux désactiver l'historique à tout moment : clic droit sur l'icône d'Oniris dans la zone de notification, puis décoche « Historique du presse-papiers ». Plus rien n'est lu ni enregistré ensuite. Les éléments déjà gardés restent jusqu'à ce que tu les supprimes ou que la purge les retire ;
- il est purgé automatiquement, au démarrage d'Oniris et après chaque copie : les éléments plus vieux que la durée choisie (7 jours par défaut) et ceux au-delà du nombre maximal (200 par défaut) sont supprimés, sauf ceux que tu as épinglés. Ici, la suppression est réelle : l'élément est effacé de la base.

## À venir

Ces fonctions n'existent pas encore. Chacune ajoutera sa propre section ici avant d'être disponible :

- comptes Google, Microsoft et Spotify : à venir ;
- Claude, via Claude Code : à venir.
