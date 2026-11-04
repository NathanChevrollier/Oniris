# Oniris — feuille de route du développement

**Spec :** [`docs/superpowers/specs/2026-11-03-oniris-design.md`](../specs/2026-11-03-oniris-design.md)

L'utilisateur veut le projet complet, sans livraison intermédiaire à valider. Le travail est quand même découpé en sept plans successifs. Chacun produit du logiciel qui fonctionne et se teste seul. Un plan n'est rédigé en détail qu'au moment où on l'attaque, parce qu'il réutilise les noms et interfaces réellement créés par les plans précédents.

| # | Plan | Contenu (sections de la spec) | Dépend de | Sortie vérifiable |
|---|---|---|---|---|
| 1 ✅ | **Fondations** ([plan](2026-11-03-oniris-01-fondations.md)) | Dépôt, pilule et panneau (§4.1, §4.2, §4.4), raccourci appui court / maintien (§4.3), détection du plein écran, système visuel (§5), SQLite (§8), paramètres, courtier d'outils avec confirmations et journal (§7.4, §7.5), banc de mesure des performances (§9) | — | L'app s'ouvre et se ferme en moins de 100 ms, les mesures de RAM et de CPU sont relevées, le choix du framework d'interface est tranché |
| 2 | **Modules locaux** | Agenda et frise, rappels avec notifications (§6.2), notes (§6.12), minuteur et focus (§6.13), presse-papiers (§6.11), état du PC (§6.9), météo (§6.10) | 1 | Les cartes affichent des données réelles ; les outils de ces modules sont enregistrés auprès du courtier |
| 3 | **Recherche, favoris, projets** | Index de fichiers en mémoire et barre de commande (§6.6), favoris avec import Nexus et navigateurs (§6.5), projets, plan, git, sessions Claude et « Reprendre » (§6.7) | 1, 2 | Recherche en moins de 50 ms mesurée ; « Reprendre » ouvre VS Code et Claude |
| 4 | **Comptes en ligne** | OAuth et premier lancement guidé, Gmail et Graph (§6.3), agendas externes (§6.2), Spotify via SMTC et Web API (§6.4), sondes des sites et passerelle MCP Zenytt (§6.8), document de confidentialité (§10) | 1, 2 | Mails unifiés, musique pilotée, sites surveillés, sans aucun envoi ni suppression réels pendant les tests |
| 5 | **IRIS — cerveaux et sécurité** | Ollama et cerveau local, Claude Code non interactif, aiguillage (§7.3), double LLM et quarantaine, provenance (§7.4), outils généraux (commandes, fichiers, applications), serveur MCP d'Oniris, briefing (§6.1), tri des mails, mémoire (§7.6), personnalité (§7.1) | 1 à 4 | Tests d'injection (mails piégés) qui échouent tous à déclencher une action ; commande simple en moins de 1 s |
| 6 | **Voix** | Push-to-talk, Whisper (Vulkan ou ROCm), Piper avec une voix féminine française, interruption de la parole (§7.2), sphère réactive, mot d'activation « IRIS » en dernier (§4.3) | 5 | Chaîne voix → action → réponse parlée, latences mesurées |
| 7 | **Finitions** | Audit de performance complet (§9, test de fuite sur 24 h), passe visuelle sur l'app réelle, paramètres et premier lancement, empaquetage NSIS (sur accord de l'utilisateur), graphify | 1 à 6 | Tous les budgets du §9 sont tenus et mesurés |

## Règles qui valent pour tous les plans

- Messages de commit au format Conventional Commits, en français, **sans aucune co-signature Claude**.
- Aucun commit ni push sans l'accord de l'utilisateur. Le mode accordé pour l'exécution est précisé au lancement.
- Ne jamais lancer `pnpm tauri build` sans son accord.
- Aucun test ne doit envoyer de vrai mail, supprimer de vraies données ni agir sur le vrai VPS.
- Tout nouveau flux de données vers un tiers est documenté dans `docs/legal/confidentialite.md` au sein du même lot de travail.
- Après chaque plan, mettre à jour le graphe graphify du projet (`/graphify --update`), conformément au CLAUDE.md global.
