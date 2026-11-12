# Phase 5 : bot Telegram DL Manager — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un bot Telegram (@socialvideodl_bot, « DL Manager ») télécharge en MP4 ou MP3 les liens qu'on lui envoie, en passant uniquement par l'API du cœur, dans son propre conteneur, ouvert à tous.

**Architecture:** Nouveau crate `crates/bot` (binaire `reclip-bot`). Un petit client Bot API maison (reqwest, long polling) et un client du cœur (reqwest, clé `delegate` + `X-Reclip-On-Behalf-Of: tg:<id>`). La logique du parcours dépend de deux traits (`Telegram`, `Core`) pour être testée avec des faux. Le cœur crée la clé du bot au démarrage et l'écrit dans un volume `secrets` partagé en lecture seule. Deux modes : API Telegram officielle (fichiers ≤ 50 Mo) par défaut, serveur Bot API local (≤ 2 000 Mo) quand `TELEGRAM_API_ID`/`TELEGRAM_API_HASH` sont fournis.

**Tech Stack:** Rust (tokio, reqwest avec multipart et rustls, serde), wiremock, Docker (cargo-chef), `aiogram/telegram-bot-api` pour le serveur local.

**Spec:** `docs/superpowers/specs/2026-11-11-reclip-platform-design.md` §2.2, §4.1, §6, §7 ; `docs/superpowers/specs/2026-11-12-phase3b-decisions.md` (#53 : bot ouvert à tous, sans liste d'accès).

## Décisions (écarts assumés par rapport à la spec)

- **Pas de teloxide** : un client Bot API d'environ 200 lignes (getUpdates, sendMessage, editMessageText, editMessageReplyMarkup, sendPhoto, sendVideo, sendAudio, answerCallbackQuery, deleteMessage). Moins de dépendances, image plus petite, tests wiremock simples, URL de l'API configurable.
- **Pas de volume `bot-files` partagé** : le bot télécharge le fichier depuis le cœur dans son dossier temporaire et l'envoie en multipart. Le serveur local accepte le multipart jusqu'à 2 000 Mo ; le mode officiel jusqu'à 50 Mo.
- **Deux modes** : sans `TELEGRAM_API_ID`, le bot parle à `https://api.telegram.org` et envoie un lien signé au-delà de 50 Mo. Avec, le profil `telegram-local` démarre `telegram-bot-api` et le bot l'utilise (limite 2 000 Mo).
- **Pas de liste d'accès** (`RECLIP_BOT_ALLOWED_USERS` supprimée, décision #53).
- **État persistant** : un petit fichier JSON (`/var/lib/reclip-bot/state.json`) garde les téléchargements suivis (tâche, chat, message, langue) pour reprendre après un redémarrage, car une clé `delegate` ne peut pas lister les tâches de tous les utilisateurs.
- **Progression** : interrogation de `GET /jobs/{id}` toutes les 3 s (pas de SSE), message édité seulement si le texte change.

## Global Constraints

- Variables du bot : `TELEGRAM_BOT_TOKEN` (obligatoire), `TELEGRAM_API_URL` (défaut `https://api.telegram.org`), `RECLIP_CORE_URL` (défaut `http://core:8899`), `RECLIP_BOT_KEY_FILE` (défaut `/run/reclip/bot.key`) ou `RECLIP_BOT_KEY` (prioritaire), `RECLIP_BOT_STATE` (défaut `/var/lib/reclip-bot/state.json`), `RECLIP_BOT_MAX_UPLOAD` (défaut : `50M` si l'URL de l'API est l'officielle, `2000M` sinon).
- Variable du cœur : `RECLIP_BOT_KEY_FILE` (vide par défaut = rien n'est créé ; le compose met `/run/reclip/bot.key`).
- Clé du bot : nom `telegram-bot`, droits `probe`, `jobs:create`, `jobs:read`, `jobs:cancel`, `delegate`, sans expiration, non `unlimited` (les limites `tg` s'appliquent par utilisateur de toute façon).
- Le bot n'appelle que l'API du cœur ; il n'a jamais accès à la base.
- Conversations privées uniquement ; groupes et canaux ignorés en silence.
- Langue : français si `language_code` commence par `fr`, anglais sinon.
- Les secrets (jeton du bot, clé du bot) ne sont jamais écrits dans les logs, même en debug ; les URL de l'API Telegram contiennent le jeton : les erreurs reqwest sont journalisées sans URL (`without_url()`).
- Fichiers ≤ ~300 lignes.
- Ne jamais lire, afficher, modifier ni committer `deploy/.env` (il contient le vrai jeton). Les tests et `setup.sh` utilisent des fichiers temporaires.

## Review Focus

1. **Un utilisateur clique sur ✖ Cancel d'un message d'un autre utilisateur** (message transféré) : refus, la tâche n'est pas touchée (le cœur répond introuvable car le propriétaire diffère). → test Task C.
2. **Fichier de plus que la limite d'envoi** : lien signé envoyé, tâche gardée, aucun envoi tenté. → test Task C.
3. **Le bot redémarre pendant un téléchargement** : au démarrage, il reprend le suivi depuis `state.json` et livre ; une tâche disparue côté cœur est retirée de l'état sans erreur. → test Task C.
4. **Lien de playlist ou publication à plusieurs vidéos** : refus poli avec renvoi vers le site (adresse publique depuis `GET /api/v1/config` ou `RECLIP_PUBLIC_URL`). → test Task C.
5. **Telegram renvoie 429 (`retry_after`)** sur une édition de progression : le bot attend et continue, sans planter ni boucler. → test Task B.

---

## Vague 1 (en parallèle)

### Task A: Clé du bot créée par le cœur

**Files:** `crates/core/src/config.rs` (+ tests), `crates/core/src/access/bot_key.rs` (nouveau), `crates/core/src/main.rs` (appel au démarrage), tests `crates/core/tests/bot_key.rs`.

**Produces:** `pub async fn ensure_bot_key(store: &Store, path: &Path) -> anyhow::Result<()>`.

- Si `RECLIP_BOT_KEY_FILE` est vide : rien.
- Si le fichier existe et contient une clé valide (vérifiée avec `verify_key`, non révoquée, avec le droit `delegate`) : rien.
- Sinon : créer la clé (décisions ci-dessus), écrire `rck_…` + `\n` dans le fichier (dossier créé au besoin ; écriture dans un fichier temporaire du même dossier puis renommage ; mode `0600` sur Unix) et journaliser « bot key written to <path> » sans la clé. L'ancienne clé invalide n'est pas révoquée automatiquement (elle peut déjà l'être).
- Tests : `no_path_creates_nothing`, `creates_a_delegate_key_once` (deux démarrages = une seule clé), `replaces_a_revoked_key`, `replaces_garbage`, `key_is_not_logged` (capturer les logs), `file_is_private` (Unix seulement).
- Commit `feat(core): create the telegram bot key at startup`.

### Task B: Crate `bot` : configuration et clients

**Files:** `crates/bot/{Cargo.toml,src/main.rs,src/config.rs,src/telegram/{mod,types,client}.rs,src/core_client.rs,src/i18n.rs}`, tests `crates/bot/tests/{telegram_client,core_client}.rs`.

**Produces:**
- `Config::from_env(get: &dyn Fn(&str) -> Option<String>) -> anyhow::Result<Config>` avec les variables des Global Constraints ; `max_upload: u64` (octets, suffixes `K`/`M`/`G`).
- Types Bot API minimaux (`Update`, `Message`, `Chat` avec `kind` privé/autre, `User` avec `language_code`, `CallbackQuery`, `InlineKeyboardMarkup`, `InlineKeyboardButton { text, callback_data }`).
- `#[async_trait]`-free trait (fonctions `async fn` dans un trait, Rust 2024) `Telegram` : `get_updates(offset, timeout_s)`, `send_message(chat, text, keyboard?) -> MessageId`, `edit_message(chat, msg, text, keyboard?)`, `send_photo_url(chat, url, caption, keyboard?) -> MessageId`, `send_video(chat, path, caption, duration?, width?, height?)`, `send_audio(chat, path, title?, performer?, duration?)`, `answer_callback(id, text?)`, `delete_message(chat, msg)`. Implémentation `HttpTelegram` (reqwest). Sur 429 : attendre `parameters.retry_after` (plafonné à 60 s) puis réessayer une fois. « message is not modified » sur une édition = succès. Erreurs sans URL (le jeton est dans l'URL).
- Trait `Core` : `config() -> PublicConfig`, `probe(user, url) -> ProbeResponse`, `create_job(user, CreateJobRequest) -> Job`, `get_job(user, id) -> Job`, `list_jobs(user) -> Vec<Job>`, `cancel_job(user, id)`, `delete_job(user, id)`, `download_file(user, id, dest: &Path) -> u64` (streaming, sans tout charger en mémoire), `signed_link(user, id) -> LinkResponse`. Implémentation `HttpCore` ; `user: i64` devient l'en-tête `X-Reclip-On-Behalf-Of: tg:<id>`. Erreurs : `CoreError { status, code: ErrorCode, message, retry_after: Option<u64> }`.
- `i18n.rs` : `enum Lang { Fr, En }`, `Lang::from_code(Option<&str>)`, et une fonction par message (accueil, aide, choix du format, qualité, progression, terminé, annulé, erreurs par `ErrorCode`, playlist refusée, lien expiré, fichier trop gros + lien).
- Tests wiremock : chaque méthode envoie la bonne requête (chemin, en-têtes, corps JSON ou multipart) ; 429 avec `retry_after: 1` → une seconde tentative réussie ; « message is not modified » → Ok ; l'erreur ne contient pas le jeton ; `CoreError` reprend `code` et `Retry-After` ; `download_file` écrit le fichier.
- Commit `feat(bot): telegram and core clients`.

### Task D: Déploiement

**Files:** `deploy/bot.Dockerfile`, `deploy/compose.yaml`, `deploy/.env.example`, `deploy/setup.sh`, `deploy/README.md` (section bot, en français), `README.md` (section « Telegram bot »), `.github/workflows/ci.yml`.

- `bot.Dockerfile` : cargo-chef, binaire `reclip-bot`, image d'exécution `gcr.io/distroless/cc-debian12:nonroot` (certificats inclus) ou `debian:bookworm-slim` + `ca-certificates` ; utilisateur non-root ; cible ~20 Mo.
- `compose.yaml` :
  - `core` : volume `secrets:/run/reclip`, variable `RECLIP_BOT_KEY_FILE: ${RECLIP_BOT_KEY_FILE:-/run/reclip/bot.key}`.
  - `bot` (profil `telegram`) : image `reclip-bot`, réseaux `core_net` et `tg_net`, volumes `secrets:/run/reclip:ro` et `bot-data:/var/lib/reclip-bot`, variables des Global Constraints avec défauts, `depends_on: core` (condition `service_healthy`), `restart: unless-stopped`.
  - `telegram-bot-api` (profil `telegram-local`) : image `aiogram/telegram-bot-api` épinglée par version, `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `TELEGRAM_LOCAL=1`, volume `tg-data`, réseau `tg_net`.
  - `core` n'est pas sur `tg_net`.
- `.env.example` : `TELEGRAM_BOT_TOKEN=`, `TELEGRAM_API_ID=`, `TELEGRAM_API_HASH=`, `TELEGRAM_API_URL=` (commentaire : `http://telegram-bot-api:8081` avec le profil `telegram-local`), `COMPOSE_PROFILES=` (commentaire : `telegram` ou `telegram,telegram-local`), `RECLIP_BOT_MAX_UPLOAD=`.
- `setup.sh` (POSIX sh) : `setup.sh [chemin du .env]` (défaut `./.env`) ; refuse d'écraser un fichier existant ; demande ou lit dans l'environnement `TELEGRAM_BOT_TOKEN`, `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `RECLIP_PUBLIC_URL` ; écrit les profils adaptés ; si un `api_id` est fourni, fait l'appel unique `logOut` vers `https://api.telegram.org/bot<jeton>/logOut` avant le passage au serveur local (sauf `--no-logout`) ; n'affiche jamais le jeton. Test : `shellcheck` en CI et un essai non interactif dans un dossier temporaire.
- CI : construire l'image du bot avec contrôle de taille (< 40 Mo), `docker compose config` avec `COMPOSE_PROFILES=telegram,telegram-local`, `shellcheck deploy/setup.sh`.
- Docs : comment créer le bot (BotFather, nom « DL Manager »), mode simple (jeton seul) et mode 2 Go (`api_id`/`api_hash` sur my.telegram.org, profil `telegram-local`, `setup.sh` ou `logOut` manuel), `RECLIP_TELEGRAM_BOT_USERNAME` pour le lien du pied de page.
- Commit `feat(deploy): telegram bot service and setup script`.

## Vague 2

### Task C: Parcours du bot

**Files:** `crates/bot/src/{app,router,flow,pending,tracker,deliver,state}.rs`, `crates/bot/src/main.rs` (boucle), tests `crates/bot/tests/flow.rs` (faux `Telegram` et faux `Core` en mémoire).

**Consumes:** traits `Telegram`, `Core`, `Config`, `i18n` de Task B.

- Boucle : `get_updates` (timeout 50 s, `allowed_updates` message + callback_query) ; chaque mise à jour traitée dans sa propre tâche ; erreur réseau → pause de 5 s ; arrêt propre sur SIGTERM.
- `/start`, `/help` ; `/mp4 <lien>`, `/mp3 <lien>` (saute le choix du format) ; `/cancel` (annule les téléchargements en cours de l'utilisateur suivis par le bot).
- Lien envoyé (premier `http(s)://` du texte) : `probe` ; vidéo → `send_photo_url` (miniature, sinon `send_message`) avec titre, durée et boutons **🎬 MP4** / **🎵 MP3** ; playlist → refus avec lien vers le site.
- Choix en attente en mémoire 30 min, identifiant court dans `callback_data` (≤ 64 octets) ; expiré → « Lien expiré, renvoyez-le ». Le clic vérifie que `from.id` est l'auteur du choix.
- MP4 : boutons de qualité depuis `formats` (une ligne par hauteur, au plus 6) + « Best » ; MP3 : téléchargement direct.
- Création de la tâche (`title` et `thumbnail` repris de l'analyse) ; message de progression édité toutes les 3 s si le texte change (pourcentage, vitesse, temps restant) avec bouton **✖ Cancel** (`x:<id de tâche>`) ; le clic vérifie l'auteur.
- Livraison : taille ≤ `max_upload` → téléchargement du fichier dans un dossier temporaire, `send_video` (durée, largeur/hauteur si connues) ou `send_audio` (titre, artiste = `uploader`), puis `delete_job` et suppression du fichier local, et suppression du message de progression ; au-delà → `signed_link` et message avec le lien (tâche gardée).
- Erreurs du cœur traduites (`rate_limited` avec l'attente, `forbidden_target`, `unavailable`, `too_large`, etc.) ; échec de la tâche → message d'erreur à la place de la progression.
- `state.rs` : `State { tracked: Vec<Tracked { job_id, user_id, chat_id, message_id, lang, format, title, uploader, duration } > }`, écrit de façon atomique à chaque changement ; au démarrage, reprend le suivi de chaque entrée ; une tâche introuvable est retirée.
- Tests (faux en mémoire, sans réseau) : `start_and_help`, `link_shows_format_choice`, `mp4_then_quality_creates_job`, `mp3_creates_job`, `progress_edits_only_on_change`, `cancel_button_cancels`, `other_users_cannot_press_buttons`, `small_file_is_sent_then_deleted`, `large_file_gets_signed_link`, `playlist_is_refused`, `expired_choice`, `group_messages_ignored`, `rate_limit_message_shows_wait`, `restart_resumes_tracking`, `vanished_job_is_dropped`, `french_and_english`.
- Commit `feat(bot): dl manager flow`.
