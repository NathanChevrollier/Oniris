# Plan 9 : vérifications à faire à la main

Les mises à jour passent par GitHub et l'installeur réel : aucun test automatique n'y a touché.

- [ ] Installer la 1.2.0 à la main (installeur NSIS).
- [ ] Publier une 1.2.1 de test sur GitHub (signée).
- [ ] Laisser tourner Oniris (3 min après le démarrage, ou « Vérifier maintenant » dans Paramètres > Mises à jour) : la pilule annonce « Mise à jour 1.2.1 prête » avec « Redémarrer » et « Plus tard ».
- [ ] « Plus tard » ferme l'annonce sans rien installer ; les réglages affichent « Version 1.2.1 prête » et ses notes.
- [ ] « Redémarrer » : l'installation passive se fait, puis Oniris se relance en 1.2.1.
- [ ] Sans cliquer : fermer Oniris installe la 1.2.1 sans relance.
- [ ] Pendant un tour IRIS ou en plein écran, « Redémarrer » attend le calme avant d'installer.
- [ ] Arrêt ou fermeture de session Windows avec une mise à jour en attente : rien n'est installé pendant l'extinction.
- [ ] Installer pendant qu'Oniris finit de se fermer : l'installation se déroule proprement, sans erreur ni fichier verrouillé.
- [ ] Après la mise à jour, `%LOCALAPPDATA%\Oniris` (réglages, base, voix) et `%USERPROFILE%\oniris-wakeword` (extraits d'apprentissage) sont intacts.
- [ ] Décocher « Mises à jour automatiques » puis Enregistrer : aucune vérification automatique ; « Vérifier maintenant » fonctionne toujours.
- [ ] Couper le réseau : aucun toast, juste une ligne `warn` dans les journaux.
