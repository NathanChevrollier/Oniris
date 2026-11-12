# Plan 8 : vérifications à faire à la main

Le mot d'activation écoute le vrai micro : aucun test automatique n'y a touché. Active-le dans la feuille IRIS, section « Voix » (case « Mot d'activation « Dis Iris » », voix installée), puis « Enregistrer la voix ». L'état « à l'écoute » doit s'afficher environ une seconde après l'enregistrement (la fenêtre relit l'état 1,2 s plus tard, l'écouteur le met à jour à sa boucle suivante).

- [ ] Dire « Dis Iris, quelle heure est-il ? » à 1 m : IRIS répond.
- [ ] Même phrase à 3 m.
- [ ] Une soirée de jeu en plein écran : aucune détection (l'écoute passe « en pause »).
- [ ] Une heure de musique et de vidéo avec l'écoute active : compter les faux déclenchements (noter le nombre).
- [ ] CPU au repos avec l'écoute active : `tools/perf/budgets.mjs` et gestionnaire des tâches, ≤ 1 % du processeur total en moyenne sur 60 s.
- [ ] Débrancher puis rebrancher le casque : l'écoute reprend (état « micro indisponible » puis « à l'écoute »).
- [ ] Mettre la voix en pause depuis la zone de notification : l'écoute s'arrête (« en pause »), et reprend à la reprise.
- [ ] Décocher la case : l'icône du micro de Windows disparaît.

## Mode apprentissage

Cocher « Mode apprentissage » (grisé tant que le mot d'activation est décoché), puis « Enregistrer la voix ». Deux petits boutons apparaissent à côté de la pilule. Dossier des extraits : `%USERPROFILE%\oniris-wakeword\recordings\learn\{pos,neg}`.

- [ ] Réveil réussi suivi d'une commande : +1 positif (compte dans les réglages, après réouverture de la feuille).
- [ ] Clic sur « Je vais dire Iris » puis « Dis Iris » sans réveil : +1 positif.
- [ ] Réveil à tort, puis clic sur « Faux réveil » dans les 10 s : +1 négatif.
- [ ] « Faux réveil » disparaît 10 s après le réveil, et n'apparaît pas pour un appui sur Alt+Espace.
- [ ] « Supprimer les extraits » (avec confirmation) vide les dossiers `pos` et `neg`, sans toucher aux autres enregistrements.
