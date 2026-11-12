# Phase 1 : décisions prises et suites à donner

Bilan de l'exécution du plan `docs/superpowers/plans/2026-11-11-phase1-foundations.md` (19 commits, `850fa5f..a1c47eb`).

## Écarts par rapport à la spécification

| Sujet | Spec | Réalité | Suite |
|---|---|---|---|
| Taille de l'image `core` | < 350 Mo | 757 Mo sur disque, 229 Mo compressés (avant : 1,04 Go / 279 Mo). ffmpeg + ffprobe statiques = 271 Mo, Deno = 114 Mo. | Compiler un ffmpeg minimal (codecs et conteneurs utiles à yt-dlp seulement). La CI bloque au-delà de 800 Mo. |
| Faux yt-dlp | `tests/fixtures/` | Binaire Rust `crates/core/src/bin/fake-ytdlp.rs` (marche sous Windows et Linux) | Aucune. |
| Volume `downloads` | volume séparé | sous `data:/data/downloads` | Aucune. |
| Test Playwright de bout en bout | §8 | absent | À ajouter en phase 2, avec la progression en direct. |

## Décisions qui pèsent sur les phases suivantes

- **Phase 2** : la colonne `jobs.format_id` reste vide en phase 1. Il faut l'enregistrer quand les tâches en attente survivront à un redémarrage.
- **Phase 2** : annulation par groupe de processus (SIGTERM puis SIGKILL), balayage au démarrage des tâches restées `queued`/`downloading`, nettoyage des fichiers expirés.
- **Phase 2** : `set_state` ne vérifie pas que l'id existe, et `finished_at` est conservé si une tâche repasse en file.
- **Phase 3** : masquer les messages `internal` côté HTTP (la dernière ligne de yt-dlp peut contenir des chemins du serveur) ; refuser les caractères de contrôle dans les URL ; JSON pour `/api`, `/api/` et les erreurs 405 ; ajouter un code d'erreur `not_found`.
- **Déploiement** : `ports: 8899` expose le service sur toutes les interfaces de l'hôte. Derrière Dokploy/Traefik, préférer le domaine et retirer la publication du port.
- **CI** : la toolchain Rust de la CI suit `stable` alors que l'image fige 1.94 ; un nouveau lint clippy peut casser la CI sans changement de code.

## Petites améliorations notées

- `export_ts.rs` : un nouveau type ajouté à `shared` doit aussi être ajouté à la liste d'export.
- Frontend : une course connue si l'on relance « Fetch » pendant des téléchargements ; dépendances `latest` dans `web/package.json` à figer par plage ; `build.ts` dépend du dossier courant.
- `.dockerignore` : remettre `**/.env*` et `cookies*.txt` par sécurité.
- README : commandes de développement en bash uniquement (ajouter l'équivalent PowerShell).
