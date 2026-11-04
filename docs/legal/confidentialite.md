# Confidentialité d'Oniris

Version 1 — 4 novembre 2026. Ce document décrit simplement ce qu'Oniris fait de tes données. Il est mis à jour à chaque nouveauté qui touche à ta vie privée.

## En bref

Oniris range tout chez toi, sur ton PC. Il n'envoie aucune statistique d'usage, et il n'y a aucun compte à créer pour l'utiliser. La seule communication avec Internet, aujourd'hui, est la météo, et seulement si tu as choisi une ville.

## Où sont tes données

Tout est stocké en local, dans le dossier `%APPDATA%\Oniris` :

- la base de données `oniris.db` : réglages, agenda, rappels, notes, minuteurs, historique du presse-papiers, journal des actions ;
- les journaux techniques (`logs`) : ils ne contiennent ni le texte de tes notes, ni celui de tes rappels, ni celui de ton presse-papiers. Seulement des identifiants, des tailles et des messages d'erreur.

Supprimer ce dossier efface tout ce qu'Oniris sait de toi.

## Pas de télémétrie

Oniris ne mesure pas ton usage et n'envoie ni rapport de plantage, ni statistique, ni identifiant à qui que ce soit.

## Météo

Si tu laisses le champ « ville » vide, la météo est désactivée et Oniris ne fait aucune requête.

Si tu choisis une ville, Oniris contacte le service gratuit [Open-Meteo](https://open-meteo.com) (sans compte ni clé) :

- il envoie le nom de la ville que tu as saisi pour trouver ses coordonnées, puis les coordonnées (latitude et longitude) pour obtenir la prévision ;
- ces échanges se font au plus une fois toutes les 30 minutes environ, quand tu ouvres le panneau, et aussi juste après chaque changement de ville ;
- Open-Meteo voit, comme tout site web, l'adresse IP de ta connexion ;
- Oniris n'envoie ni ton nom, ni ton adresse e-mail, et s'identifie seulement par « Oniris/0.1 ».

Pour arrêter, vide le champ « ville » : le lieu et la dernière météo mémorisés sont effacés.

## Historique du presse-papiers

L'historique reste sur ton PC ; il n'est envoyé nulle part. Ses limites :

- les copies faites depuis un gestionnaire de mots de passe qui les signale comme sensibles sont exclues ;
- en revanche, les **extensions de gestionnaires de mots de passe dans le navigateur ne signalent pas leurs copies** : un mot de passe copié depuis une telle extension peut donc être enregistré dans l'historique ;
- tu peux désactiver l'historique dans les réglages à tout moment ;
- il est purgé automatiquement : les éléments plus vieux que la durée choisie (7 jours par défaut) et ceux au-delà du nombre maximal (200 par défaut) sont supprimés, sauf ceux que tu as épinglés.

## À venir

Ces fonctions n'existent pas encore. Chacune ajoutera sa propre section ici avant d'être disponible :

- comptes Google, Microsoft et Spotify : à venir ;
- Claude, via Claude Code : à venir.
