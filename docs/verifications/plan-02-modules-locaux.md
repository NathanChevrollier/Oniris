# Plan 2 : vérifications à faire à la main

Les modules du plan 2 (agenda, rappels, notes, minuteur, presse-papiers, état du PC, météo) touchent à Windows : notifications, presse-papiers, compteurs GPU, veille. Aucun test n'a été lancé sur ton bureau réel. Ouvre toi-même les applications dont tu as besoin (Bloc-notes, Paint, navigateur). Compte environ 45 minutes.

Les cases marquées « ⚠️ à vérifier en priorité » sont les plus risquées.

## Lancer l'app en mode développement

Dans `Projets/Oniris` :

```powershell
pnpm tauri dev
```

Ouvre le panneau avec Alt+Espace. Pour voir les logs, ouvre le dossier `%APPDATA%\Oniris\logs` (fichier `oniris.log.<date>`).

## 1. Journal

- [ ] Dans la carte Agenda, supprime un événement : une carte de confirmation apparaît et ouvre le panneau. Rien n'est supprimé avant ton accord.
- [ ] Après avoir approuvé la suppression, annule-la depuis le journal : l'événement revient dans la liste et sur la frise.
- [ ] Annule de même la création d'un événement, la création d'un rappel et la complétion d'un rappel : la liste et la frise se rafraîchissent à chaque fois.

## 2. Agenda et frise

- [ ] Dans le champ de commande, tape « demain 14h-15h dentiste » : un événement « dentiste » apparaît demain de 14 h à 15 h, avec un repère sur la frise.
- [ ] Un événement qui finit à minuit n'apparaît pas le lendemain.
- [ ] Une journée entière s'affiche de minuit à minuit.
- [ ] Le calendrier du mois : les jours avec événements sont marqués, et un clic sur un jour affiche sa liste.
- [ ] Laisse le panneau ouvert après minuit (ou change l'heure du PC) : le jour affiché suit la date.

## 3. Rappels

- [ ] Tape « demain appeler la banque » dans « Nouveau rappel… » : il apparaît avec le jour de demain et une échéance à 9 h.
- [ ] Crée un rappel dans 2 minutes : une notification « Rappel » arrive à l'heure (60 s de retard au plus).
- [ ] ⚠️ à vérifier en priorité : crée un rappel dans 2 minutes, quitte Oniris, attends que l'échéance passe, puis relance l'app. Une notification « Rappel en retard » apparaît une seule fois. Quitte et relance encore : plus aucune notification.
- [ ] ⚠️ à vérifier en priorité : crée un rappel dans 3 minutes, mets le PC en veille avant l'échéance, puis réveille-le après l'échéance. La notification arrive dans la minute qui suit le réveil.
- [ ] Coche un rappel : il est barré et vert, et il reste jusqu'à minuit puis disparaît le lendemain. Un clic sur la coche le rouvre. Aucune notification n'est envoyée pour lui.
- [ ] Un rappel du jour met un point jaune sur la frise. Une échéance à moins de 2 h est en jaune dans la liste.

## 4. Notes

- [ ] Crée une note avec des accents (par exemple « Réunion été à Noël ») : elle apparaît dans la carte.
- [ ] La recherche trouve cette note en tapant « reunion ete » sans accents, et aussi en tapant « réunion ».
- [ ] Épingle une note : la punaise jaune s'affiche sur la ligne et n'est pas coupée. La note épinglée reste en haut.
- [ ] Au survol d'une note, les actions apparaissent. Le champ de saisie grandit jusqu'à 6 lignes.
- [ ] Une recherche sans résultat affiche l'état vide. Sans aucune note, l'état vide s'affiche aussi.

## 5. Minuteur et focus

- [ ] Clique sur « 25 min » : le minuteur démarre, l'anneau avance chaque seconde, mm:ss et le libellé sont au centre.
- [ ] Tape « minuteur 10 min thé » : le libellé est « thé ». Tape « bientôt » : le message « durée introuvable » s'affiche.
- [ ] Lance « 5 min » puis « 25 min » : les deux sont dans la liste, et la croix du second l'annule.
- [ ] Lance un minuteur de 1 min : le compte à rebours mm:ss s'affiche dans la pilule avant la sphère, puis disparaît à la fin. Une notification « terminé » apparaît.
- [ ] Lance un minuteur de 1 min, quitte Oniris, relance après plus d'1 min : une seule notification « Terminé pendant ton absence », sans pause automatique. Relance encore : aucune notification.
- [ ] Lance « Focus » : à la fin, notification « Focus terminé — pause de 5 min » et la pause démarre. À la fin de la pause, notification « Pause terminée » et rien ne démarre ensuite.

## 6. Presse-papiers

- [ ] Copie un texte depuis le Bloc-notes : l'entrée apparaît en haut de la carte en 1 s environ. Copie le même texte : pas de doublon, l'entrée remonte avec une heure à jour.
- [ ] Copie une image (Win+Maj+S, ou une image depuis Edge) : une miniature apparaît, avec les bonnes couleurs et dans le bon sens.
- [ ] Copie un mot de passe depuis l'application de bureau 1Password : rien n'est enregistré, et le log indique `copie exclue par sa source`.
- [ ] Attention : une copie faite par l'**extension de navigateur** d'un gestionnaire de mots de passe (1Password, Bitwarden dans Edge ou Chrome) **sera enregistrée**, car elle passe par le presse-papiers ordinaire du navigateur. Teste-le avec ton extension. Si ça te gêne, désactive l'historique : mets `clipboard_enabled` à faux dans les paramètres. Copie ensuite quelque chose : rien n'est enregistré. Remets-le à vrai : c'est de nouveau enregistré.
- [ ] ⚠️ à vérifier en priorité : ouvre le Bloc-notes toi-même et clique dans son champ de texte. Ouvre le panneau avec Alt+Espace, puis clique une entrée de texte du presse-papiers. Le panneau se ferme et le texte est collé dans le Bloc-notes. Cette copie n'est pas enregistrée une seconde fois.
- [ ] ⚠️ à vérifier en priorité : ouvre le panneau depuis le Bloc-notes, passe à une autre application avec Alt+Tab, reviens, puis clique une entrée. Rien n'est collé dans la mauvaise fenêtre (le contenu reste seulement dans le presse-papiers).
- [ ] Ouvre le panneau en cliquant sur la pilule, puis clique une entrée : le contenu est mis dans le presse-papiers sans être collé. Colle-le toi-même avec Ctrl+V.
- [ ] Avec Paint ouvert et au premier plan, clique une entrée image : l'image est collée avec les bonnes couleurs.
- [ ] Après un collage par clic, la touche Ctrl n'est pas restée bloquée et le raccourci Alt+Espace fonctionne toujours.
- [ ] Au survol d'une entrée, les boutons épingler et supprimer apparaissent. La suppression est définitive. Une entrée épinglée survit au nettoyage.
- [ ] Règle la rétention à 1 jour et le maximum à 20 éléments, puis copie plus de 20 textes : les plus anciens non épinglés sont purgés, les épinglés restent.
- [ ] Copie un très long texte (environ 10 Mo) : l'entrée est tronquée (elle finit par `…`), et ni Oniris ni l'application source ne se bloquent.
- [ ] Les logs ne contiennent jamais le texte copié, seulement des lignes comme `texte de N octets`.

## 7. Ce PC

- [ ] La carte « Ce PC » affiche CPU, GPU, RAM et VRAM, qui bougent chaque seconde. Compare avec le Gestionnaire des tâches.
- [ ] ⚠️ à vérifier en priorité : sur la RX 7800 XT, la jauge GPU suit l'onglet « GPU » du Gestionnaire (moteur 3D) et la jauge VRAM suit la « Mémoire GPU dédiée » (environ 16 Go au total). Lance un jeu ou une vidéo 4K pour la faire bouger.
- [ ] Dans les logs, il n'y a pas de ligne « PDH indisponible ». Si elle est là, les jauges GPU et VRAM doivent être masquées, sans plantage.
- [ ] Ferme le panneau : l'activité d'Oniris dans le Gestionnaire retombe. Rouvre-le : un seul rythme d'une seconde, pas de doublon.
- [ ] La valeur « Assistant » est proche de la mémoire privée de `oniris.exe` et de `msedgewebview2.exe` réunies (colonne « Mémoire privée »).

## 8. Météo

- [ ] Sans ville, un lien « Choisir une ville » s'affiche sous l'horloge. Clique-le, tape « Paris » puis Entrée : la météo (icône, température, libellé, ville) apparaît en quelques secondes.
- [ ] Échap dans le champ de ville l'annule sans fermer le panneau. Un clic sur le nom de la ville permet de la changer.
- [ ] Tape une ville inconnue : pas de plantage, le badge reste sur « Météo… ».
- [ ] Coupe le Wi-Fi ou débranche le câble : la dernière météo reste affichée. Au bout de plus de 3 h hors ligne, elle est grisée avec « à HHhMM ». L'ouverture du panneau reste instantanée, sans attendre le réseau.
- [ ] Reconnecte le réseau : la météo se met à jour.
- [ ] Vide le champ de ville : le badge redevient « Choisir une ville ».
- [ ] Ni ville ni coordonnées n'apparaissent dans les logs.

## 9. Panneau

- [ ] Sur un écran plus petit (1080p, mise à l'échelle 125 à 150 %), la grille défile à la molette et la barre de commande reste visible avec le focus.
- [ ] Les cartes « Bientôt » sont compactes et le défilement est visible.
- [ ] Les épingles jaunes des notes et du presse-papiers ne sont pas coupées.
- [ ] Les états vides s'affichent sans événement, sans note, sans élément copié, et quand une recherche ne trouve rien.
- [ ] Sur ton 2560×1440, rien ne déborde et l'ouverture reste fluide.

## 10. Logs

- [ ] Le dossier `%APPDATA%\Oniris\logs` existe et contient `oniris.log.<date>` avec des lignes.
- [ ] Il ne contient ni texte de note, ni contenu copié, ni ville, ni mot de passe. Fais une recherche sur un mot de tes notes pour t'en assurer.

## 11. Performance

Suis la procédure de [`tools/perf/README.md`](../../tools/perf/README.md), avec l'écoute du presse-papiers active et le panneau fermé. Ne touche pas au PC pendant la mesure.

- [ ] Le CPU au repos reste à environ 0 %, sans régression par rapport au plan 1 (l'écoute du presse-papiers ne se réveille qu'à une copie).
- [ ] L'ouverture du panneau reste sous 100 ms et la RAM sous 200 Mo.

Envoie-moi le tableau affiché ou le fichier `tools/perf/results-*.json`.

## Si quelque chose ne va pas

Note le numéro de la case (par exemple 3.4) et ce que tu as vu, puis envoie-le-moi. Les points les plus incertains sont le collage dans la bonne fenêtre (6.5 et 6.6), les jauges GPU (7.2), le rappel échu pendant la fermeture (3.3) et le réveil de veille (3.4). Les cases sont numérotées par section, dans l'ordre.
