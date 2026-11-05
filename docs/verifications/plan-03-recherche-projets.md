# Plan 3 : vérifications à faire à la main

Les modules du plan 3 (recherche, favoris, projets) touchent à Windows : parcours de ton disque, OneDrive, lancement de VS Code et de Claude, requêtes d'icônes. Aucun test n'a été lancé sur ton bureau réel, aucune application n'a été ouverte. Ouvre toi-même ce dont tu as besoin (VS Code, ton navigateur). Compte environ 45 minutes.

Les cases marquées « ⚠️ à vérifier en priorité » sont les plus risquées.

## Lancer l'app en mode développement

Dans `Projets/Oniris` :

```powershell
pnpm tauri dev
```

Ouvre le panneau avec Alt+Espace. Pour voir les logs, ouvre le dossier `%APPDATA%\Oniris\logs` (fichier `oniris.log.<date>`).

Les chiffres de performance ne se mesurent pas ici : ils viennent du build de mesure (section 15 et [`tools/perf/README.md`](../../tools/perf/README.md)), pas de `pnpm tauri dev`.

Avant de commencer, choisis tes dossiers de projets : sans eux, la carte Projets te propose de les choisir. Sans dossier de recherche choisi, la recherche indexe ton dossier utilisateur.

## 1. Recherche : fichier, application, projet

- [ ] Tape le début du nom d'un fichier que tu connais : il apparaît dans la liste. Entrée l'ouvre avec son application par défaut, et le panneau se ferme.
- [ ] Tape le nom d'une application du menu Démarrer (par exemple « code » ou « obs ») : elle apparaît avec « Application ». Entrée la lance.
- [ ] Tape le nom d'un de tes projets : il apparaît comme projet.
- [ ] Ctrl+Entrée sur un fichier ouvre son dossier, pas le fichier.
- [ ] Tape un nom avec une faute de frappe ou en désordre (« rapport facture » pour `facture_rapport.pdf`) : le fichier est quand même trouvé.
- [ ] Un fichier exécutable (`.exe`, `.bat`) ne se lance pas directement : une carte de confirmation apparaît d'abord. Refuse : rien ne se lance.
- [ ] Les fichiers que tu ouvres souvent remontent en tête des résultats après quelques usages.

## 2. Ctrl+P et préfixes

- [ ] Ouvre le panneau et tape Ctrl+P : le champ prend le focus et contient `p `, et le navigateur n'ouvre pas la boîte d'impression. Tape le début d'un projet : seuls les projets apparaissent.
- [ ] Les préfixes `f ` (fichiers), `a ` (applications), `b ` (favoris) et `n ` (notes) limitent les résultats à ce type.
- [ ] Une note trouvée par la recherche : Entrée met la carte Notes en évidence.

## 3. Commandes « / »

- [ ] Tape `/` : la liste des 7 commandes apparaît (minuteur, focus, note, rappel, événement, météo, presse-papiers).
- [ ] `/minuteur 10 min thé`, `/note idée`, `/rappel demain appeler la banque` et `/événement demain 14h dentiste` créent chacun leur élément dans la bonne carte.
- [ ] `/focus` démarre une session focus ; `/presse-papiers` ouvre la carte du presse-papiers.
- [ ] Une commande inconnue (`/zzz`) affiche « Commande inconnue — essaie /note, /rappel… ».

## 4. Recherche pendant l'indexation

- [ ] Juste après le lancement d'Oniris, ouvre le panneau et tape un nom : un indicateur d'indexation s'affiche et la saisie ne se bloque pas. Quand l'indexation finit, les résultats se complètent.
- [ ] Pendant l'indexation, le CPU de ton PC reste raisonnable et le reste du bureau ne ralentit pas.
- [ ] Crée un fichier avec un nom unique dans un dossier indexé : il est trouvé en quelques secondes, sans relancer. Supprime-le : il disparaît des résultats.

## 5. OneDrive

- [ ] ⚠️ à vérifier en priorité : si des dossiers (Documents, Bureau, Images) sont dans OneDrive, les fichiers « à la demande » (icône de nuage) sont trouvés par leur nom **sans être téléchargés** : leur icône reste un nuage et OneDrive ne se met pas à télécharger.
- [ ] ⚠️ à vérifier en priorité : comportement avec la redirection des dossiers connus de OneDrive (Known Folder Move, vérification reportée pendant le développement). Cherche un fichier de ton dossier Documents et un du Bureau : chacun est trouvé une seule fois (pas de doublon entre `C:\Users\toi\Documents` et `OneDrive\Documents`) et s'ouvre.
- [ ] Les liens symboliques et jonctions ne provoquent ni boucle infinie ni doublons.

## 6. Pas de console qui clignote

- [ ] ⚠️ à vérifier en priorité : ouvre le panneau et laisse la carte Projets s'afficher (elle lance `git`). Aucune fenêtre noire de console n'apparaît, même une fraction de seconde. Ouvre et ferme le panneau 5 fois de suite.
- [ ] Lance une recherche, ouvre une application et un fichier : aucune fenêtre de console ne s'ouvre.
- [ ] Clique « Reprendre » (section 10) : seules VS Code, et Windows Terminal en cas de repli, s'ouvrent, sans console supplémentaire.

## 7. Favoris et import

- [ ] « Importer » puis « Depuis les navigateurs » : les signets de Chrome, Edge et Firefox (si installés) apparaissent dans la carte Favoris. Teste Firefox ouvert, puis fermé : l'import marche dans les deux cas.
- [ ] Un deuxième import ne crée pas de doublons.
- [ ] « Depuis un export Nexus… » : choisis un export JSON de Nexus_Dashboard, ses favoris apparaissent avec leurs catégories.
- [ ] Choisis un fichier JSON qui n'est pas un export Nexus : un message d'erreur clair s'affiche, sans plantage.
- [ ] Ajoute un favori à la main avec le bouton « + » : il apparaît, et un clic l'ouvre dans ton navigateur.

## 8. Favicons

- [ ] Après un import, les initiales se remplacent peu à peu par les vraies icônes, à raison d'environ une par seconde.
- [ ] Un site sans icône garde ses initiales, sans plantage ni message d'erreur.
- [ ] Un favori vers une adresse de ton réseau local (par exemple `http://192.168.1.1`) garde ses initiales : Oniris ne la contacte pas.
- [ ] Les logs ne contiennent pas les adresses des sites.

## 9. Glisser-déposer

- [ ] Glisse un favori sur un autre : l'ordre change, et il est gardé après avoir fermé et rouvert le panneau, puis après avoir relancé Oniris.
- [ ] Au clavier : sur un favori, Alt+Flèche gauche ou droite le déplace.
- [ ] Un favori supprimé disparaît de la carte.

## 10. Projets et « Reprendre »

- [ ] La carte Projets liste tes projets avec leur branche git, le nombre de fichiers modifiés et leur dernière activité. Un projet sans git s'affiche sans erreur.
- [ ] Un projet avec un plan (`docs/superpowers/plans/*.md`) affiche son avancement et la tâche en cours.
- [ ] Le bouton « … » d'un projet permet de l'épingler (il passe en tête) ou de le masquer, et le choix est gardé après un redémarrage. « Tous les projets (N) » ouvre la recherche limitée aux projets.
- [ ] ⚠️ à vérifier en priorité : « Reprendre » sur un projet qui a déjà une session Claude Code. VS Code s'ouvre **sur le bon dossier**, puis Claude se lance dans le terminal intégré sur **la dernière session** (pas une nouvelle), avec la consigne de reprise.
- [ ] ⚠️ à vérifier en priorité : le lien `vscode://` est encodé deux fois (vérifié seulement par simulation, jamais avec le vrai VS Code). Avec un chemin qui contient des espaces et des accents, VS Code ouvre bien le bon dossier et la consigne arrive intacte (accents, apostrophes).
- [ ] Repli : si VS Code ne se lance pas par le lien, Windows Terminal s'ouvre avec `claude --continue`. La consigne n'y est pas transmise : c'est voulu.
- [ ] Sur un projet sans session Claude, « Reprendre » ouvre VS Code et lance Claude sans erreur.
- [ ] Ctrl+Entrée sur un projet dans la recherche fait la même chose que « Reprendre ».
- [ ] Si VS Code est introuvable, un message clair s'affiche au lieu d'un plantage.

## 11. Mode orchestrateur

- [ ] Le bouton « Orchestr. » d'un projet ouvre VS Code et lance Claude avec la consigne du mode orchestrateur (celle de « Reprendre » suivie des règles d'orchestration), pas seulement celle de « Reprendre ».
- [ ] Message de réglage « ~600 caractères » (vérification reportée) : si l'écran des paramètres existe à ce moment-là (prévu au plan 7), colle une consigne très longue pleine d'accents (é, è, à, ç, œ). Le refus doit venir à un moment cohérent avec « max ~600 caractères selon les accents ». La limite est annoncée de façon un peu optimiste pour un texte accentué : note à partir de combien de caractères ça refuse.

## 12. Cas particuliers

- [ ] Dossier au nom bizarre : crée dans un de tes dossiers de projets un dossier `Mon projet été & co's` (espaces, accents, apostrophe, `&`). Il apparaît comme projet, « Reprendre » l'ouvre dans VS Code, et aucune commande étrange ne se lance.
- [ ] Un fichier dont le nom contient `%`, `&` ou `^` s'ouvre normalement depuis la recherche.
- [ ] Un dossier de projets qui contient beaucoup de sous-dossiers (`node_modules`) ne ralentit pas l'ouverture du panneau.
- [ ] Un projet sur un disque réseau ou débranché ne bloque pas le panneau.

## 13. Panneau

- [ ] Sur ton 2560×1440, le panneau tient en entier avec toutes les cartes, sans débordement ni coupure. Une capture faite avec un aperçu navigateur (sans lancer l'app) est correcte.
- [ ] Sur un écran plus petit (1080p), la grille défile à la molette et la barre de commande reste visible.
- [ ] Le grand espace vide sous les cartes « En lecture » et « Mails » est attendu : ce sont des cartes « Bientôt » compactes, le plan 4 les remplira.

## 14. Logs et vie privée

- [ ] Les logs ne contiennent aucun chemin de fichier, aucun nom de favori, aucun titre de session Claude : seulement des compteurs et des identifiants.
- [ ] Le document de confidentialité (`docs/legal/confidentialite.md`) correspond à ce que tu observes.

## 15. Performance

Le test de performance de la recherche existe, dans `crates/core/src/search/rank.rs`. Il construit un index de 300 000 entrées et mesure 20 requêtes.

- [x] ⚠️ à vérifier en priorité : mesure sous 50 ms. Mesuré le 5 novembre 2026 en release (`cargo test -p oniris-core --release search_perf -- --ignored --nocapture`) : **médiane 10,2 ms** (minimum 3,1, maximum 25,6) sans historique d'usage, **médiane 9,9 ms** (minimum 3,7, maximum 27,1) avec 5 000 usages. Le pire cas reste sous 50 ms. Reste à confirmer le ressenti sur ton vrai disque (case suivante).
- [ ] Ressenti réel : après l'indexation, tape trois lettres dans la barre de commande. La liste se met à jour sans à-coup.
- [ ] Le CPU au repos reste à environ 0 % panneau fermé, une fois l'indexation terminée.
- [ ] L'ouverture du panneau reste sous 100 ms et la RAM sous 200 Mo, avec l'index en mémoire (build de mesure, voir [`tools/perf/README.md`](../../tools/perf/README.md)).

## Si quelque chose ne va pas

Note le numéro de la case (par exemple 10.4) et ce que tu as vu, puis envoie-le-moi. Les points les plus incertains sont « Reprendre » (10.4 et 10.5), OneDrive (5.1 et 5.2), la console invisible (6.1) et la mesure de recherche (15.1). Les cases sont numérotées par section, dans l'ordre.
