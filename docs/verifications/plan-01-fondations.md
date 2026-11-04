# Plan 1 : vérifications à faire à la main

Pendant le développement, aucun test n'a été lancé sur ton bureau réel : pas de frappes simulées et pas de fenêtres ouvertes. Ces points demandent donc un vrai clavier et un vrai écran. Compte environ 15 minutes.

## Lancer l'app en mode développement

Dans `Projets/Oniris` :

```powershell
pnpm tauri dev
```

La pilule apparaît en haut de l'écran. Pour voir les messages de débogage, ouvre la console du terminal. Pour ouvrir les outils de développement d'une fenêtre, fais un clic droit dessus, puis « Inspecter ».

## 1. Raccourci Alt+Espace

Mets l'Explorateur Windows au premier plan, puis vérifie :

- [ ] Un appui court sur Alt+Espace ouvre le panneau, un second appui le ferme.
- [ ] Un maintien d'environ 1 s affiche exactement une ligne `ptt start` puis une ligne `ptt end` dans la console.
- [ ] Après avoir relâché les touches, la barre de menu de l'Explorateur ne s'active pas (pas de lettres soulignées) et le menu système « Restaurer / Déplacer / Fermer » ne s'ouvre pas.
- [ ] Si tu maintiens Alt+Espace, que tu relâches Alt avant Espace, puis que tu tapes sur Espace, cette touche fonctionne normalement.
- [ ] La touche Espace seule, dans un champ de texte, n'est pas interceptée. Ctrl+Alt+Espace non plus.

## 2. Panneau

- [ ] Le panneau descend avec son animation et les cartes apparaissent en cascade.
- [ ] Le champ de commande a le focus à chaque ouverture.
- [ ] Échap ferme le panneau, et un clic à côté du panneau aussi. Un clic dans le panneau ne le ferme pas.
- [ ] Le panneau se ferme quand tu passes à une autre fenêtre, par Alt+Tab ou par un clic.
- [ ] Un clic sur la pilule ou sur l'icône de la zone de notification, panneau ouvert, le ferme sans scintillement, même si tu cliques lentement.

## 3. Pilule

- [ ] La pilule est centrée en haut de l'écran et sa sphère est immobile au repos.
- [ ] Un survol de la pilule d'environ 0,35 s ouvre le panneau.
- [ ] Quand le panneau se ferme alors que la souris est encore sur la pilule, il ne se rouvre pas tout seul.
- [ ] En plein écran (vidéo YouTube en F11, jeu en fenêtre sans bordure), la pilule disparaît, puis elle revient à la sortie du plein écran.
- [ ] Avec une fenêtre simplement agrandie, la pilule reste visible.

## 4. Système

- [ ] Lancer Oniris une deuxième fois ne crée pas de seconde instance : le panneau de l'instance déjà ouverte s'affiche.
- [ ] L'icône « Oniris » est présente dans la zone de notification. Un clic gauche bascule le panneau, le menu fonctionne, et « Quitter » ferme l'app.
- [ ] Aucune erreur « not allowed by ACL » n'apparaît dans la console de la pilule ni dans celle du panneau.

## 5. Mesure des performances

Suis la procédure de [`tools/perf/README.md`](../../tools/perf/README.md). Pendant la mesure, le panneau s'ouvre et se ferme une trentaine de fois. Ne touche pas au PC pendant ce temps.

Le résultat décide du framework d'interface :
- si l'ouverture se fait en moins de 100 ms et que la RAM reste sous 200 Mo, on garde React ;
- sinon, je te présente les chiffres avant de lancer le plan 2.

Envoie-moi le tableau affiché ou le fichier `tools/perf/results-*.json`.

## Si quelque chose ne va pas

Note le numéro de la case et ce que tu as vu, puis envoie-le-moi. Le point le plus incertain est la case 1.3 (le menu de l'Explorateur qui s'active). Une solution de repli est déjà prévue pour ce cas.
