# ReClip : plateforme Rust, API, WebMCP et bot Telegram

- **Date** : 2026-11-11
- **Statut** : validé section par section en conversation, délégation complète pour la suite
- **Branche** : `feat/rust-platform` (part de `fix/youtube-bot-check`)

## 1. Contexte et objectifs

ReClip est un fork de `averygan/reclip` : un backend Flask de 210 lignes qui lance yt-dlp, et une page unique `templates/index.html` de 705 lignes (HTML, CSS et JS mélangés). L'application est déployée sur un serveur (Dokploy probable, d'après le nommage Swarm des logs), publique et sans protection.

Ce que veut le propriétaire :

1. Une base de code organisée, sans fichiers trop longs, où Rust, TypeScript, Python et Bun collaborent proprement.
2. Une infrastructure légère : Rust là où il apporte un gain mesurable.
3. Pouvoir annuler ou supprimer un téléchargement en cours.
4. Ne rien perdre en rafraîchissant la page.
5. Une API publique avec clés, utilisable par des agents IA, protégée contre les abus sans gêner un usage intensif normal (par exemple 12 liens collés d'un coup).
6. WebMCP.
7. Un bot Telegram (« DL Manager », `@socialvideodl_bot`) qui télécharge en MP3 et MP4, avec un bouton sur le site.
8. Un dossier `deploy/` dont le compose remplit lui-même les variables utiles.

### Mesures de départ

| Mesure | Valeur |
|---|---|
| Mémoire au repos (Flask + gunicorn) | 38 Mo |
| Mémoire pendant une analyse YouTube | 94 Mo (surtout yt-dlp et Deno) |
| Image Docker | 1,04 Go |
| Binaire yt-dlp autonome (`yt-dlp_linux`) | 39 Mo, fonctionne sans Python, embarque `yt_dlp_ejs` |

### Hors périmètre

- Comptes utilisateurs, inscription, paiement.
- Interface web de gestion des clés (ligne de commande et routes admin suffisent).
- Bot dans les groupes Telegram.
- Traduction de l'interface web (reste en anglais ; le bot parle français et anglais).
- Refonte visuelle du site.

## 2. Architecture

### 2.1 Langages et rôles

| Langage | Rôle |
|---|---|
| Rust | Serveur `core` (axum, tokio) et bot `bot` (teloxide). Types partagés dans `crates/shared`. |
| TypeScript | Interface web et outils WebMCP, dans `web/`. |
| Bun | Compilation, serveur de développement et tests de `web/`. |
| Python | Uniquement à l'intérieur du binaire autonome yt-dlp. Aucun code Python à nous. |
| Deno | Runtime JavaScript utilisé par yt-dlp pour les challenges YouTube (Bun est déprécié par yt-dlp, voir yt-dlp#16766). |

La collaboration Rust ↔ TypeScript passe par `ts-rs` : les types de `crates/shared` génèrent `web/src/api/types.gen.ts`. La CI échoue si le fichier généré n'est pas à jour.

### 2.2 Arborescence

```
reclip/
├── Cargo.toml              workspace
├── crates/
│   ├── shared/             types API, codes d'erreur, définitions des outils MCP
│   ├── core/               serveur web + API + tâches + MCP
│   └── bot/                bot Telegram DL Manager
├── web/                    TypeScript, compilé par Bun vers web/dist (servi par core)
├── deploy/
│   ├── compose.yaml
│   ├── .env.example
│   ├── core.Dockerfile
│   ├── bot.Dockerfile
│   ├── setup.sh
│   └── README.md
├── tests/fixtures/         faux yt-dlp pour les tests
├── assets/                 captures, avatar Telegram
└── docs/
```

Règle de taille : aucun fichier source au-delà de ~300 lignes ; au-delà, on découpe par responsabilité.

### 2.3 Organisation interne de `core`

```
crates/core/src/
├── main.rs            démarrage, configuration, sous-commandes CLI (serve, keys …)
├── config.rs          lecture des variables d'environnement et valeurs par défaut
├── http/              routeur axum, une sous-module par groupe de routes
├── jobs/              file, exécution, progression, annulation, nettoyage
├── ytdlp/             construction des arguments, lecture de la progression, classement des erreurs
├── auth/              cookie visiteur, clés API, jetons admin, liens signés
├── limits.rs          limites de débit (governor)
├── netguard.rs        refus des adresses internes (SSRF)
├── store/             SQLite (sqlx) : tâches, clés, réglages
└── mcp/               serveur MCP sur /mcp
```

## 3. Le cœur

### 3.1 API v1

Toutes les routes sont sous `/api/v1`, en JSON.

| Route | Rôle |
|---|---|
| `GET /api/v1/config` | Configuration publique : nom du bot Telegram, limites, version, jeton d'essai WebMCP. |
| `POST /api/v1/probe` | Analyse un lien. Réponse `{kind: "video", …}` (titre, miniature, durée, auteur, qualités) ou `{kind: "playlist", entries: [...]}`, plafonnée par `RECLIP_PLAYLIST_LIMIT` (corrige upstream #90). |
| `POST /api/v1/jobs` | Crée une tâche : `url`, `format` (`mp4` ou `mp3`), `format_id` facultatif, `title` facultatif. Répond `202` avec la tâche. |
| `GET /api/v1/jobs` | Tâches du demandeur, les plus récentes d'abord. |
| `GET /api/v1/jobs/{id}` | Une tâche. |
| `GET /api/v1/jobs/{id}/events` | Flux SSE : changements d'état et progression. |
| `POST /api/v1/jobs/{id}/cancel` | Annule une tâche en attente ou en cours. |
| `DELETE /api/v1/jobs/{id}` | Annule si besoin, supprime fichier et tâche. |
| `GET /api/v1/jobs/{id}/file` | Fichier, avec prise en charge de `Range`. |
| `POST /api/v1/jobs/{id}/link` | Lien signé temporaire `/d/{jeton}` (utilisé par le bot au-delà de 2 Go). |
| `GET /d/{jeton}` | Téléchargement par lien signé, sans clé, valable `RECLIP_LINK_TTL`. |
| `/api/v1/admin/keys` | Gestion des clés (création, liste, révocation), avec `RECLIP_ADMIN_TOKEN`. |
| `GET /healthz` | Santé du service. |
| `/mcp` | Serveur MCP (section 5.3). |

Les anciennes routes `/api/info`, `/api/playlist`, `/api/download`, `/api/status/{id}` et `/api/file/{id}` disparaissent.

### 3.2 Tâche

Champs : `id` (UUID v7), `owner`, `url`, `site` (domaine normalisé), `format`, `format_id`, `title`, `state`, `progress` (0 à 100), `speed`, `eta`, `filename`, `size`, `error_code`, `error_message`, `created_at`, `finished_at`, `expires_at`.

États :

```
queued → downloading → processing → done → expired
   │          │             │
   └──────────┴─────────────┴──→ cancelled | failed
```

### 3.3 Exécution

- **File d'attente** : une tâche n'est jamais refusée parce que des téléchargements sont en cours ; elle attend.
- **Concurrence globale** : `RECLIP_MAX_CONCURRENT` (4 par défaut).
- **Concurrence par site** : `RECLIP_MAX_PER_SITE` (2 par défaut). Limite le risque que YouTube ou Instagram signalent l'IP du serveur après une rafale.
- **Équité** : la file prend les tâches à tour de rôle entre demandeurs, pour qu'un lot de 50 liens n'en bloque pas un autre.
- **Processus** : yt-dlp est lancé dans son propre groupe de processus (`setsid`), avec `nice 10` pour garder le site réactif. Arguments : ceux de la version Python (cookies, proxy, runtime JS) + `--newline --progress-template` en JSON + `--max-filesize` + `--match-filter "duration <= RECLIP_MAX_DURATION"`.
- **Progression** : lue ligne par ligne sur la sortie de yt-dlp, diffusée en SSE au plus 2 fois par seconde.
- **Annulation** : `SIGTERM` sur le groupe (yt-dlp et ffmpeg), puis `SIGKILL` après 5 s, puis suppression du dossier de la tâche.
- **Délai maximal** : `RECLIP_JOB_TIMEOUT` (30 min), traité comme une annulation avec l'erreur `timeout`.
- **Fichiers** : un dossier par tâche, `downloads/{id}/`. Le nettoyage supprime les fichiers après `RECLIP_FILE_TTL` (24 h) et passe la tâche à `expired`. Les tâches expirées sont effacées de la base après 7 jours.
- **Redémarrage** : au démarrage, les tâches `downloading` ou `processing` passent à `failed` avec le code `interrupted`, et leurs dossiers sont supprimés. Les tâches `queued` restent dans la file.

### 3.4 Codes d'erreur

`bot_check`, `unsupported_url`, `unavailable`, `private`, `geo_blocked`, `too_large`, `too_long`, `timeout`, `interrupted`, `cancelled`, `rate_limited`, `forbidden_target`, `invalid_request`, `internal`.

Le classement lit la dernière ligne `ERROR:` de yt-dlp. Pour `bot_check`, le message reprend l'aide de la version Python (configurer ou renouveler `RECLIP_COOKIES`).

### 3.5 Stockage

SQLite dans le volume `data`, migrations versionnées (`sqlx migrate`). Tables : `jobs`, `api_keys`, `settings` (adresse publique déduite, secrets générés). Mode WAL.

## 4. Accès et limites

### 4.1 Demandeurs

| Demandeur | Identification | Propriétaire des tâches |
|---|---|---|
| Visiteur du site | Cookie `reclip_client` (aléatoire, `HttpOnly`, `SameSite=Lax`, `Secure` derrière HTTPS) | `web:<id>` |
| Clé API | `Authorization: Bearer rck_<id>_<secret>` | `key:<id>` |
| Utilisateur du bot | Clé du bot + `X-Reclip-On-Behalf-Of: tg:<user_id>` (droit `delegate`) | `tg:<user_id>` |

Un demandeur ne voit et ne modifie que ses propres tâches. Les requêtes basées sur le cookie qui modifient quelque chose exigent un en-tête `Origin` égal à l'origine du site.

### 4.2 Clés API

- Format `rck_<id>_<secret>` : `id` de 8 caractères, `secret` de 32 octets aléatoires en base62. Seul le SHA-256 du secret est stocké.
- Droits : `probe`, `jobs:create`, `jobs:read`, `jobs:cancel`, `delegate`.
- Options : expiration, limites propres, `unlimited` (pour la clé du propriétaire).
- Gestion : `reclip-core keys create|list|revoke` dans le conteneur, ou `/api/v1/admin/keys` avec `RECLIP_ADMIN_TOKEN`.
- Suivi : `last_used_at` et compteur de requêtes par clé.

### 4.3 Limites par défaut

Pensées pour un usage intensif normal : un lot de 12 liens (10 Instagram + 2 YouTube) passe sans aucun refus ; il est simplement réparti dans la file.

| Demandeur | Analyses | Tâches créées | Tâches en file ou en cours |
|---|---|---|---|
| Visiteur (par IP) | 120 / min | 100 / heure | 30 |
| Clé API (par clé) | 120 / min | 200 / heure | 50 |
| Utilisateur Telegram | 30 / min | 40 / heure | 10 |
| Clé `unlimited` | aucune | aucune | aucune |

Une limite dépassée renvoie `429` avec `Retry-After` et les en-têtes `RateLimit-*`. Toutes les valeurs sont réglables par variables d'environnement.

Plafonds par fichier : `RECLIP_MAX_FILESIZE` (2 Go), `RECLIP_MAX_DURATION` (3 h).

### 4.4 Vraie IP

Toutes les requêtes arrivent par le proxy de la plateforme (`10.0.1.9` dans les logs). L'IP est lue dans `X-Forwarded-For` uniquement si la requête vient d'une adresse de `RECLIP_TRUSTED_PROXIES` (plages privées par défaut).

### 4.5 Protections

- **SSRF** : seuls `http` et `https` sont acceptés ; l'hôte est résolu et refusé s'il pointe vers une adresse privée, locale, de lien local ou de métadonnées (`forbidden_target`).
- **Réseaux** : `core` n'est pas sur le réseau du serveur Bot API. C'est aussi ce qui couvre la limite du contrôle SSRF : il ne vérifie que l'URL de départ, pas les redirections que yt-dlp suit ensuite.
- **Site privé** : `RECLIP_PUBLIC_WEB=false` exige une clé pour toute l'API.
- **En-têtes** : CSP stricte, `X-Content-Type-Options`, `Referrer-Policy`, `Permissions-Policy`.

## 5. Interface web, WebMCP et MCP

### 5.1 Frontend

```
web/
├── index.html
├── styles/            base.css, form.css, cards.css
├── src/
│   ├── main.ts
│   ├── api/           client.ts, types.gen.ts
│   ├── state.ts
│   ├── ui/            input.ts, format-toggle.ts, card.ts, progress-bar.ts, footer.ts
│   ├── progress.ts    SSE et reconnexion
│   └── webmcp.ts
└── tests/
```

TypeScript sans framework. Même apparence qu'aujourd'hui (Instrument Serif, DM Mono, accent `#e85d2a`).

Comportement :
- Barre de progression avec pourcentage, vitesse et temps restant.
- Bouton **Cancel** pendant le téléchargement, **Remove** une fois terminé.
- Au chargement : `GET /api/v1/jobs`, reconstruction des cartes, reprise des flux SSE. Le brouillon du champ de liens est gardé dans `localStorage`.
- Messages d'erreur selon le code, y compris « réessayez dans X min » pour `rate_limited`.
- Lien Telegram sous la liste des sites : icône Telegram (SVGL), texte « Use on Telegram », cible `https://t.me/<RECLIP_TELEGRAM_BOT_USERNAME>`, masqué si la variable est vide.

### 5.2 WebMCP

- `document.modelContext.registerTool()` si disponible, rien sinon.
- Jeton d'essai Chrome injecté depuis `RECLIP_WEBMCP_TRIAL_TOKEN` (balise `<meta http-equiv="origin-trial">`).
- Outils : `probe_media`, `start_download`, `list_downloads`, `get_download`, `cancel_download`.
- Ils passent par le même état que les boutons : les cartes apparaissent à l'écran. Ils utilisent le cookie du visiteur ; aucune clé n'est exposée.

### 5.3 Serveur MCP `/mcp`

- Transport HTTP « streamable » (crate `rmcp`), authentification par clé API.
- Mêmes 5 outils, mêmes noms, descriptions et schémas : la définition vit dans `crates/shared` et est exportée en JSON pour `web/`.
- `get_download` renvoie l'URL du fichier ; pour un agent, un lien signé.

## 6. Bot DL Manager

### 6.1 Fonctionnement

- Rust (teloxide), long polling, via le serveur Bot API local (`telegram-bot-api`, mode `--local`), fichiers jusqu'à 2 Go.
- Appelle uniquement l'API du cœur, avec sa clé `delegate`.
- Fichier : le bot récupère le fichier du cœur dans le volume `bot-files`, partagé avec le serveur Bot API, puis l'envoie par chemin local. Il supprime ensuite la tâche dans le cœur et le fichier local.
- Au-delà de 2 Go : lien signé `/d/{jeton}`.

### 6.2 Parcours

1. `/start` : message d'accueil.
2. Un lien envoyé : titre, miniature, durée, boutons **🎬 MP4** / **🎵 MP3**.
3. MP4 : boutons de qualité + « Best ».
4. Message de progression édité toutes les 3 s environ, bouton **✖ Cancel**.
5. Livraison : `sendVideo` (lecture en continu, miniature, durée) ou `sendAudio` (titre, artiste).

Commandes : `/mp4 <lien>`, `/mp3 <lien>`, `/cancel`, `/help`. Langue : français si `language_code` commence par `fr`, anglais sinon. Conversations privées uniquement. Les playlists sont refusées avec un renvoi vers le site. Liste blanche facultative `RECLIP_BOT_ALLOWED_USERS`.

### 6.3 Robustesse

- Au démarrage, le bot liste les tâches de ses utilisateurs via le cœur, reprend le suivi et livre.
- Choix MP4/MP3 en attente gardés 30 min en mémoire ; au-delà ou après redémarrage : « Lien expiré, renvoyez-le ».

## 7. Déploiement

### 7.1 Compose

| Service | Profil | Réseaux | Volumes |
|---|---|---|---|
| `core` | (toujours) | `core_net` | `data`, `downloads`, `secrets` |
| `bot` | `telegram` | `core_net`, `tg_net` | `bot-files`, `secrets` (lecture seule) |
| `telegram-bot-api` | `telegram` | `tg_net` | `bot-files`, `tg-data` |

Toutes les variables ont une valeur par défaut `${VAR:-…}` dans le compose. `deploy/.env.example` documente chacune.

### 7.2 Valeurs automatiques

- **Adresse publique** : `RECLIP_PUBLIC_URL` si défini ; sinon déduite de la première requête passée par un proxy de confiance (`X-Forwarded-Proto` + `X-Forwarded-Host`), enregistrée dans `settings`, affichée dans les logs.
- **Secrets** générés au premier démarrage s'ils sont absents, gardés dans `data` : jeton admin (affiché une fois dans les logs), clé de signature des liens. La clé API du bot est en plus écrite dans le volume `secrets` (`secrets/bot.key`), le seul que le bot peut lire : il n'a jamais accès à la base.
- **À fournir** : `TELEGRAM_BOT_TOKEN`, `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `COMPOSE_PROFILES=telegram`, et en option `RECLIP_COOKIES`.

### 7.3 Images

- `core` : construction Rust (cargo-chef) + Bun pour `web/` ; exécution sur `debian:bookworm-slim` avec `yt-dlp_linux`, Deno 2.5.6, ffmpeg statique épinglé, utilisateur non-root, `HEALTHCHECK` sur `/healthz`, `yt-dlp -U` au démarrage sauf `RECLIP_NO_UPDATE`. Objectif : moins de 350 Mo.
- `bot` : binaire Rust sur image minimale, environ 20 Mo.

### 7.4 `deploy/setup.sh`

Génère un `.env` complet pour un hôte sans Dokploy, et fait l'appel unique `logOut` vers l'API Telegram officielle avant le passage au serveur local.

### 7.5 Migration

- Dokploy : passer l'application en type « Compose » sur `deploy/compose.yaml`. Étapes dans `deploy/README.md`.
- Suppression du code Python (`app.py`, `ytdlp_options.py`, `templates/`, `requirements.txt`, `pytest.ini`, `tests/`, `reclip.sh`, `Dockerfile`, `docker-compose.yml`, `docker-entrypoint.sh`) à la fin de la phase 1.

## 8. Tests et CI

| Niveau | Contenu |
|---|---|
| Rust unitaire | Arguments yt-dlp, lecture de progression, classement des erreurs, clés, limites, SSRF, liens signés. |
| Rust intégration | Serveur réel + faux yt-dlp (`RECLIP_YTDLP_BIN`, script dans `tests/fixtures/`) : file, concurrence par site, annulation, délai, redémarrage, propriété des tâches, `/mcp`. |
| Bot | `wiremock` pour Telegram et le cœur : parcours complet, annulation, reprise. |
| Web | `bun test` : client, découpage des liens, reconstruction après rafraîchissement, schémas WebMCP. |
| Bout en bout | Playwright : coller, télécharger, rafraîchir, annuler. |
| Manuel | Téléchargement YouTube réel ; bot réel. |

CI GitHub Actions : `cargo fmt --check`, `clippy -D warnings`, `cargo test`, contrôle de `types.gen.ts`, vérification TypeScript, `bun test`, Playwright, construction des deux images avec contrôle de taille, `docker compose config`.

## 9. Phases de livraison

Chaque phase a son plan d'implémentation et se termine par une version déployable.

1. **Fondations** : workspace, `shared`, `core` à parité (probe, tâches simples, fichiers, cookies/proxy/runtime, codes d'erreur), `web/` découpé en TypeScript, `deploy/` (service `core`), CI, suppression du Python.
2. **Cycle de vie** : file avec concurrence globale et par site, SSE, annulation, suppression, reprise après rafraîchissement, nettoyage, redémarrage.
3. **Accès et limites** : cookie visiteur et propriété, clés API, admin, limites, vraie IP, SSRF, en-têtes, liens signés.
4. **Agents** : WebMCP et `/mcp`.
5. **Bot** : DL Manager, serveur Bot API local, profil `telegram`, `setup.sh`.
