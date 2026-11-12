# Phase 3b : décisions prises et suites à donner

Bilan de l'exécution du plan `docs/superpowers/plans/2026-11-12-phase3b-ui.md` sur la branche `feat/phase3b-ui`. Exécution en vagues parallèles (worktrees), un modèle par tâche : Sonnet (contrats, socle React, formats, comptes admin, `/admin`), Opus (stockage et historique, page publique, revue finale), Haiku (documentation et CI, relectures courtes).

## Décisions prises en cours de route

| Décision | Pourquoi | Coût si c'était le mauvais choix |
|---|---|---|
| Exécution en parallèle dans des worktrees, fusion par le contrôleur | Délai demandé très court | Quelques conflits résolus à la fusion |
| Faststart et retrait des métadonnées faits par un passage ffmpeg explicite après le téléchargement (`ytdlp/finalize.rs`, `RECLIP_FFMPEG_BIN`) | yt-dlp saute ses post-traitements quand le fichier est déjà au bon format | Une copie de flux en plus par fichier (disque, pas de réencodage) |
| `--remux-video` gardé pour MKV/MOV ; faststart décidé d'après l'extension réelle ; `-dn` | Honorer le conteneur choisi même pour une source en un seul fichier | Remux réel non testé hors faux binaires |
| « Remove » sur une carte terminée supprime le fichier mais garde la ligne dans l'historique (état `expired`) | L'historique de 30 jours doit survivre à la liste des cartes | `GET /jobs/{id}` renvoie la ligne expirée au lieu de 404 |
| Historique : seulement les téléchargements terminés (`filename` présent) | Les échecs et annulations ne sont pas des téléchargements passés | Aucun |
| Plafond disque : seuls les dossiers de tâches comptent ; éviction seulement si ce qui est évictable couvre l'excès | Ne jamais toucher un dossier étranger ni vider le disque pour rien | Les tâches en file attendent la fin de celles en cours |
| Suppression avant reprise : la ligne est effacée d'abord, puis le téléchargement arrêté, puis le dossier | Une reprise concurrente ne peut plus relancer une tâche supprimée | Aucun |
| Mot de passe admin par défaut valable jusqu'au changement obligatoire, puis plus jamais ; ce changement ferme toutes les autres sessions | Éviter de s'enfermer dehors si l'onglet se ferme | Fenêtre courte où le mot de passe par défaut reste utilisable |
| Échec de connexion : même coût Argon2 quel que soit le cas ; tentative comptée avant vérification | Ni fuite par le temps de réponse, ni contournement par requêtes parallèles | Un tiers peut bloquer un email admin 15 min (le jeton admin reste utilisable) |
| Contrat admin : mauvais mot de passe actuel = 400 ; 401 = session absente ou expirée | L'interface distingue une session expirée | Aucun |
| Base de police déplacée de `html` à `body` | Les tailles `rem` correspondent aux maquettes | Aucun |
| Fond des fenêtres : `#8a8780` à 85 % | Une vraie superposition doit laisser voir la page | Légèrement plus clair que la maquette |
| Zones tactiles mobiles portées à 44 px | Contrainte globale prioritaire sur les hauteurs des maquettes | Contrôles un peu plus hauts sur téléphone |
| Bundle public de 127 Ko compressés accepté (estimation : 50 Ko) | React, Radix et icônes | Premier chargement plus lent sur réseau lent |
| Fenêtres admin identiques sur tablette et ordinateur | Les maquettes montrent la même fenêtre à 768 px | Aucun |

## À faire plus tard

- CI : le contrôle d'imitation Chrome devrait ignorer les lignes « unavailable » (`grep -i chrome | grep -vqi unavailable`).
- Tester sur l'image réelle : remux webm vers MOV/MKV et `-map 0 -dn -c copy` (vérifiés seulement avec les faux binaires).
- Alléger le bundle public (chargement différé des icônes et des composants Radix).
- Le job `publish` reconstruit l'image déjà construite par le job `docker`.
- Petites dettes : fichiers de tests proches de 300 lignes, test du repli de `RECLIP_MAX_DISK`, import circulaire `downloads.ts` ↔ `history.ts`, `aria-live` sur l'état des cartes, test du nonce du Select (texte seulement), test e2e de la reprise qui ne vérifie pas la réutilisation des octets partiels (le test Rust le fait).
- Cache de taille disque : une double soustraction est possible (corrigée à la mesure suivante) ; entrée « queued » fantôme possible après suppression et reprise simultanées.
- Test `runner::cancel_stops_download` sensible à la charge de la machine.
