# Phase 2 : Cycle de vie des tâches — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Les téléchargements passent par une file équitable et bornée, affichent leur progression en direct, peuvent être annulés ou retirés, survivent à un rafraîchissement de page et à un redémarrage du serveur, et les fichiers expirés sont nettoyés.

**Architecture:** Un `JobManager` dans `reclip-core` remplace `spawn_job` : il ordonnance les tâches (limite globale, limite par site, tour de rôle entre propriétaires), lance yt-dlp dans son propre groupe de processus en lisant sa progression ligne à ligne, et publie un instantané de chaque tâche dans un « hub » (dernière valeur + diffusion) que la route SSE relaie. SQLite garde les transitions d'état ; la progression reste en mémoire. Un cookie visiteur anonyme rattache les tâches à leur propriétaire pour pouvoir les lister après un rafraîchissement.

**Tech Stack:** Rust (axum 0.8 SSE, tokio, tokio-util `CancellationToken`, tokio-stream, `libc` sur unix), sqlx sqlite, TypeScript + Bun, `EventSource`, Playwright (`@playwright/test`) pour le bout en bout.

**Spec:** `docs/superpowers/specs/2026-11-11-reclip-platform-design.md` (sections 3.1, 3.2, 3.3, 4.1 pour le cookie visiteur, 5.1, 8 ; phase 2 de la section 9). Suites de la phase 1 : `docs/superpowers/notes/2026-11-11-phase1-followups.md`.

## Global Constraints

- Aucun fichier source au-delà de ~300 lignes.
- Types d'API uniquement dans `crates/shared` ; `web/src/api/types.gen.ts` régénéré (`UPDATE_TS=1 cargo test -p reclip-shared`), jamais édité à la main.
- Défauts : `RECLIP_MAX_CONCURRENT=4`, `RECLIP_MAX_PER_SITE=2`, `RECLIP_FILE_TTL=86400` s, `RECLIP_CLEANUP_INTERVAL=600` s ; tâches `expired` effacées de la base après 7 jours ; `RECLIP_JOB_TIMEOUT=1800` s inchangé.
- Une tâche n'est jamais refusée parce que d'autres tournent : elle attend.
- Annulation : SIGTERM au groupe de processus, SIGKILL après 5 s, puis suppression de `downloads/<id>/`. Sous Windows (développement) : arrêt du processus seul.
- Progression diffusée au plus 2 fois par seconde par tâche.
- Cookie visiteur `reclip_client` : 128 bits aléatoires en hexadécimal, `HttpOnly`, `SameSite=Lax`, `Path=/`, `Max-Age` 1 an, `Secure` si la requête est en HTTPS (`X-Forwarded-Proto: https` depuis un proxy de confiance, ou schéma https). Propriétaire stocké `web:<id>`.
- Une tâche d'un autre propriétaire répond `404` (jamais `403`, pour ne pas révéler son existence).
- Apparence du site inchangée hors ajouts (barre de progression, boutons Cancel/Remove).
- Commits terminés par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Deux onglets du même visiteur** : les deux voient les mêmes tâches ; une annulation dans l'un apparaît dans l'autre via SSE. → test dans Task 5 (deux abonnés SSE reçoivent `cancelled`).
2. **Annuler une tâche encore en file** (pas encore lancée) : elle passe `cancelled` sans jamais lancer yt-dlp et libère sa place. → test dans Task 4.
3. **Annuler deux fois, ou annuler une tâche terminée** : la seconde fois répond `200` avec l'état actuel, sans erreur ni effet. → test dans Task 5.
4. **Connexion SSE coupée puis rouverte** (rafraîchissement, réseau) : le premier message reçu est l'instantané courant, pas seulement les changements suivants. → test dans Task 5.
5. **Redémarrage pendant un téléchargement** : la tâche devient `failed`/`interrupted` et son dossier disparaît ; les tâches `queued` repartent avec leur qualité choisie. → test dans Task 6.

---

## File Structure

| Fichier | Responsabilité |
|---|---|
| `crates/shared/src/api.rs` | + champs de `Job`, `CreateJobRequest.thumbnail`, `JobList`. |
| `crates/core/migrations/0002_owner_progress.sql` | Colonnes `owner`, `thumbnail` ; index. |
| `crates/core/src/store/mod.rs` (+ `store/queries.rs` si > 300 lignes) | Nouvelles requêtes. |
| `crates/core/src/http/identity.rs` | Cookie visiteur, extracteur `Owner`. |
| `crates/core/src/ytdlp/progress.rs` | Lecture des lignes de progression. |
| `crates/core/src/ytdlp/process.rs` | Lancement en groupe, arrêt gracieux. |
| `crates/core/src/ytdlp/runner.rs` | `download` en flux avec annulation. |
| `crates/core/src/jobs/{mod,hub,queue,worker,recovery,cleanup}.rs` | Gestion des tâches (remplace `jobs.rs`). |
| `crates/core/src/http/{jobs,events}.rs` | Routes liste, annulation, suppression, SSE. |
| `web/src/{api/client,state,progress,storage}.ts`, `web/src/ui/{card,progress-bar}.ts`, `web/src/main.ts` | Frontend. |
| `web/e2e/*` | Tests Playwright. |

---

### Task 1: Types partagés, migration et stockage

**Files:**
- Modify: `crates/shared/src/api.rs`, `crates/shared/tests/export_ts.rs` (liste d'export), `web/src/api/types.gen.ts` (régénéré), `crates/core/src/store/mod.rs`, `crates/core/src/store/tests.rs`
- Create: `crates/core/migrations/0002_owner_progress.sql`

**Interfaces:**
- Produces:
  - `Job` gagne : `format_id: Option<String>`, `thumbnail: Option<String>`, `progress: Option<f32>` (0 à 100), `speed: Option<u64>` (octets/s, `#[ts(type = "number | null")]`), `eta: Option<u64>` (secondes, idem), `expires_at: Option<String>` (RFC 3339).
  - `CreateJobRequest` gagne `thumbnail: Option<String>` avec `#[serde(default)]` et `#[ts(optional)]` ; ajouter aussi `#[serde(default)]` + `#[ts(optional)]` à `format_id` et `title` (corrige la remarque de phase 1 : les clients n'ont plus à envoyer `null`).
  - `struct JobList { jobs: Vec<Job> }`.
  - Store : `insert(&self, job: &Job, owner: &str)`, `get_owned(&self, id, owner) -> Result<Option<Job>>`, `list_by_owner(&self, owner, limit: u32) -> Result<Vec<Job>>` (les plus récentes d'abord, hors `expired`), `owner_of(&self, id) -> Result<Option<String>>`, `delete(&self, id) -> Result<()>`, `queued(&self) -> Result<Vec<(Job, String)>>` (avec propriétaire, ordre `created_at`), `mark_interrupted(&self) -> Result<Vec<String>>` (passe `downloading`/`processing` à `failed` + erreur `interrupted` « Interrupted by a server restart », renvoie les ids), `set_expires_at(&self, id, at: &str)`, `expire_due(&self, now: &str) -> Result<Vec<(String, Option<PathBuf>)>>` (tâches `done` avec `expires_at <= now` → `expired`, `file_path` à NULL, renvoie id et ancien chemin), `purge_expired(&self, before: &str) -> Result<u64>`.
  - `set_state` vérifie `rows_affected() == 1` (sinon erreur « job not found ») et remet `finished_at` à NULL quand l'état n'est pas terminal (deux remarques de phase 1).
  - Les champs `progress`, `speed`, `eta` ne sont pas stockés : le store renvoie `None`.

- [ ] **Step 1: Write the failing tests** (`store/tests.rs`) : `insert_keeps_owner_format_id_and_thumbnail`, `get_owned_hides_other_owners` (`get_owned(id, "web:b")` → `None` pour une tâche de `web:a`), `list_by_owner_newest_first_without_expired`, `queued_returns_jobs_with_owner_in_creation_order`, `mark_interrupted_fails_running_jobs` (états `downloading` et `processing` → `failed` + code `interrupted` ; `queued` et `done` intacts), `expire_due_and_purge` (`done` avec `expires_at` passé → `expired`, chemin renvoyé, `file_path` vide ; `purge_expired` supprime les lignes `expired` avec `finished_at` antérieur), `set_state_unknown_id_errors`, `requeue_clears_finished_at`. Dans `crates/shared/tests/serde.rs` : `create_job_request_accepts_missing_optionals` (`{"url":"u","format":"mp3"}` se désérialise).
- [ ] **Step 2: Run** `cargo test -p reclip-core store:: && cargo test -p reclip-shared` — Expected: FAIL.
- [ ] **Step 3: Implement**. Migration `0002` : `ALTER TABLE jobs ADD COLUMN owner TEXT NOT NULL DEFAULT ''`, `ADD COLUMN thumbnail TEXT`, `CREATE INDEX idx_jobs_owner_created ON jobs (owner, created_at)`. Régénérer `types.gen.ts`. Adapter les appelants existants (`jobs.rs`, tests d'API) avec le propriétaire `""` pour garder la compilation ; Task 2 les remplace.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat: job owner, live fields and store queries`

---

### Task 2: Cookie visiteur et propriété des tâches

**Files:**
- Create: `crates/core/src/http/identity.rs`
- Modify: `crates/core/src/app.rs`, `crates/core/src/http/{mod,jobs,files}.rs`, `crates/core/src/jobs.rs`, `crates/core/tests/api.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - `struct Owner(pub String)` : extracteur axum (`FromRequestParts`) qui lit le cookie `reclip_client` ; s'il est absent ou invalide (pas exactement 32 caractères hexadécimaux minuscules), il en crée un. Le middleware `identity_layer` (`axum::middleware::from_fn`) pose `Set-Cookie` sur la réponse quand un cookie a été créé.
  - Toutes les routes `/api/v1/jobs*` utilisent `Owner` : création avec ce propriétaire, lecture/fichier via `get_owned` (`404` sinon).
  - `GET /api/v1/jobs` → `JobList` (`list_by_owner`, limite 100).
  - `Secure` posé si `X-Forwarded-Proto: https` (sans contrôle de proxy de confiance en phase 2 : noté pour la phase 3).

- [ ] **Step 1: Write the failing tests** (`tests/api.rs`, client `reqwest` avec `cookie_store(true)`) : `first_request_sets_client_cookie` (attributs `HttpOnly`, `SameSite=Lax`, `Path=/`, valeur 32 hex), `invalid_cookie_is_replaced`, `jobs_are_listed_per_visitor` (le visiteur A crée 2 tâches, B en crée 1 ; A liste 2, B liste 1), `other_visitor_gets_404` (B lit, télécharge le fichier d'une tâche de A → 404 `code` `invalid_request`), `secure_cookie_behind_https_proxy`. Adapter les tests existants pour utiliser un client avec cookies.
- [ ] **Step 2: Run** `cargo test -p reclip-core --test api` — Expected: FAIL.
- [ ] **Step 3: Implement**. Génération aléatoire avec `rand` (`OsRng`) ou `getrandom`.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): anonymous visitor identity and job ownership`

---

### Task 3: Progression yt-dlp, groupe de processus et annulation

**Files:**
- Create: `crates/core/src/ytdlp/progress.rs`, `crates/core/src/ytdlp/process.rs`
- Modify: `crates/core/src/ytdlp/{mod,args,runner}.rs`, `crates/core/src/bin/fake-ytdlp.rs`, `crates/core/tests/runner.rs`, `crates/core/Cargo.toml` (`tokio-util`, `libc` en `[target.'cfg(unix)'.dependencies]`)

**Interfaces:**
- Produces:
  - `download_args` ajoute `--progress-template` `download:[reclip] %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s %(progress.eta)s` (déjà avec `--newline`).
  - `enum ProgressEvent { Download { percent: Option<f32>, speed: Option<u64>, eta: Option<u64> }, Processing }` et `fn parse_line(line: &str) -> Option<ProgressEvent>` : une ligne `[reclip] a b c d e` (valeurs `NA` = absentes ; pourcentage = a / (b sinon c) × 100, borné à 100) ; une ligne qui commence par `[Merger]`, `[ExtractAudio]`, `[VideoConvertor]` ou `[FixupM3u8]` → `Processing`.
  - `process.rs` : `fn spawn_group(cmd: &mut tokio::process::Command) -> io::Result<Child>` (unix : `process_group(0)`) et `async fn terminate(child: &mut Child, grace: Duration)` (unix : `killpg(SIGTERM)`, attente `grace`, puis `killpg(SIGKILL)` ; ailleurs : `start_kill`).
  - `runner::download(cfg, job_dir, format, format_id, url, on_progress: impl FnMut(ProgressEvent) + Send, cancel: CancellationToken) -> Result<PathBuf, ApiError>` : lit stdout ligne à ligne, appelle `on_progress`, garde stderr et les lignes stdout pour le classement (R6 de la phase 1), annule sur `cancel` (→ `ApiError` code `cancelled`, message « Download cancelled ») ou sur délai (→ `timeout`), toujours via `terminate(…, 5 s)`.
  - `fake-ytdlp` : en téléchargement, imprime 5 lignes `[reclip] …` (20 %, 40 % … 100 %, total 1000 octets) puis `[Merger] …` en MP4 ou `[ExtractAudio] …` en MP3 ; mode `slow` : une ligne `[reclip]` toutes les 100 ms pendant 30 s ; nouveau mode `spawns-child` (unix) : lance un `sleep 60` enfant et l'attend, pour vérifier l'arrêt du groupe.

- [ ] **Step 1: Write the failing tests** : unitaires `progress.rs` (`parses_known_total`, `uses_estimate_when_total_missing`, `na_values_are_none`, `clamps_to_100`, `postprocessor_lines_mean_processing`, `other_lines_are_ignored`) ; `tests/runner.rs` : `download_reports_progress_then_processing` (pourcentages croissants, dernier événement `Processing`), `cancel_stops_download` (annulation après 300 ms sur `slow` → `cancelled` en moins de 7 s, dossier sans fichier final), `timeout_still_reported` (inchangé), et sous `#[cfg(unix)]` `cancel_kills_whole_process_group` (mode `spawns-child` : après annulation, le PID de l'enfant, écrit par le faux binaire dans `job_dir/child.pid`, n'existe plus — `kill(pid, 0)` échoue).
- [ ] **Step 2: Run** `cargo test -p reclip-core` — Expected: FAIL.
- [ ] **Step 3: Implement**. `probe` garde `output()` mais passe aussi par `spawn_group`/`terminate` sur délai.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS (le test unix est ignoré sous Windows, il tournera en CI).
- [ ] **Step 5: Commit** `feat(core): yt-dlp progress, process groups and cancellation`

---

### Task 4: JobManager : hub, file équitable et exécution

**Files:**
- Create: `crates/core/src/jobs/{mod,hub,queue,worker}.rs` (supprime `crates/core/src/jobs.rs`)
- Modify: `crates/core/src/{app,config,main,lib}.rs`, `crates/core/src/http/jobs.rs`
- Test: tests unitaires dans `queue.rs` et `hub.rs`, `crates/core/tests/jobs.rs`

**Interfaces:**
- Consumes: Tasks 1 à 3.
- Produces:
  - Config : `max_concurrent: usize` (`RECLIP_MAX_CONCURRENT`, 4), `max_per_site: usize` (`RECLIP_MAX_PER_SITE`, 2), `file_ttl: Duration` (`RECLIP_FILE_TTL`, 86400), `cleanup_interval: Duration` (`RECLIP_CLEANUP_INTERVAL`, 600).
  - `fn site_of(url: &str) -> String` : hôte en minuscules sans `www.`/`m.`/`music.` ; `youtu.be` → `youtube.com`.
  - `hub.rs` : `struct Hub` avec `fn publish(&self, job: Job)` (met à jour l'instantané et diffuse ; pour une même tâche, les mises à jour de progression sont limitées à une toutes les 500 ms, les changements d'état passent toujours), `fn snapshot(&self, id) -> Option<Job>`, `fn subscribe(&self, id) -> Option<(Job, broadcast::Receiver<Job>)>`, `fn forget(&self, id)`.
  - `queue.rs` : `struct Queue` (pure, sans E/S) avec `fn push(&mut self, job_id, owner, site)`, `fn remove(&mut self, job_id) -> bool`, `fn next(&mut self, running_total: usize, running_per_site: &HashMap<String, usize>) -> Option<String>` : respecte les deux limites ; tour de rôle entre propriétaires (le propriétaire servi en dernier passe après les autres) ; au sein d'un propriétaire, ordre d'arrivée ; une tâche dont le site est saturé est sautée sans bloquer celles d'autres sites.
  - `struct JobManager` (Clone, dans `AppState`) : `async fn create(&self, owner, req) -> Result<Job, ApiError>` (insère `queued` avec `expires_at` NULL, publie, programme), `async fn cancel(&self, owner, id) -> Result<Job, ApiError>` (en file : retiré, `cancelled` ; en cours : `CancellationToken` ; terminé : renvoie l'état tel quel ; 404 si absent ou autre propriétaire), `async fn delete(&self, owner, id) -> Result<(), ApiError>` (annule si besoin, supprime le dossier et la ligne, `hub.forget`), `fn subscribe(…)`, `async fn get(&self, owner, id)` (instantané du hub s'il existe, sinon store), `async fn list(&self, owner)` (store, puis instantanés du hub par-dessus).
  - `worker.rs` : exécute une tâche : `downloading` → progression → `processing` → `done` (avec `expires_at = maintenant + file_ttl`) ou `failed`/`cancelled` (dossier supprimé) ; libère sa place et relance l'ordonnancement.
  - `http/jobs.rs` passe par `JobManager` ; `spawn_job` disparaît.

- [ ] **Step 1: Write the failing tests** : `queue.rs` : `respects_global_limit`, `respects_per_site_limit`, `skips_saturated_site_without_blocking_others`, `round_robin_between_owners` (A pousse a1 a2 a3, B pousse b1 → ordre a1 b1 a2 a3 avec limite globale 1), `remove_queued_job` ; `hub.rs` : `subscriber_gets_snapshot_then_updates`, `progress_updates_are_throttled_but_state_changes_are_not` ; `tests/jobs.rs` (faux yt-dlp, `max_concurrent` 1) : `second_job_waits_for_the_first` (b reste `queued` tant que a (`slow`) tourne), `cancel_queued_job_never_runs` (b annulé en file → `cancelled`, a continue), `cancel_running_job` (`slow` → `cancelled`, dossier supprimé), `done_job_has_expiry`, `per_site_limit_applies` (`max_concurrent` 4, `max_per_site` 1, deux URL `https://slow.example/1` et `/2` : la seconde reste `queued`).
- [ ] **Step 2: Run** `cargo test -p reclip-core` — Expected: FAIL.
- [ ] **Step 3: Implement**. L'état partagé de l'ordonnancement (file + compteurs) est protégé par un `Mutex` ; aucun `await` pendant qu'il est tenu.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): fair job queue with live hub`

---

### Task 5: Routes liste, SSE, annulation et suppression

**Files:**
- Create: `crates/core/src/http/events.rs`
- Modify: `crates/core/src/app.rs`, `crates/core/src/http/jobs.rs`, `crates/core/tests/api.rs`

**Interfaces:**
- Consumes: Tasks 2 et 4.
- Produces:
  - `GET /api/v1/jobs/{id}/events` : SSE (`text/event-stream`), événements nommés `job` dont `data` est le JSON d'un `Job`. Premier événement = instantané courant. Fermeture après un état terminal. Si la tâche est terminale dès l'ouverture : un seul événement puis fermeture. Keep-alive toutes les 15 s. Abonné en retard (`Lagged`) : renvoyer l'instantané courant et continuer.
  - `POST /api/v1/jobs/{id}/cancel` → `200 Job` (idempotent).
  - `DELETE /api/v1/jobs/{id}` → `204`.
  - En-tête `X-Accel-Buffering: no` sur la réponse SSE (proxys).

- [ ] **Step 1: Write the failing tests** : `events_stream_snapshot_then_done` (lecture du flux brut avec `reqwest` : premier `event: job` à l'état `queued` ou `downloading`, dernier à `done`, puis fin de flux), `events_for_finished_job_send_one_and_close`, `two_subscribers_see_cancellation`, `reconnect_starts_with_current_snapshot` (ouvrir, fermer, rouvrir pendant `slow` : le premier événement a `progress` non nul ou l'état `downloading`), `cancel_is_idempotent` (deux `cancel` → `200` et état `cancelled` ; `cancel` sur une tâche `done` → `200` état `done`), `delete_removes_job_and_file` (`204`, puis `GET` → `404`, dossier absent), `events_of_other_visitor_404`.
- [ ] **Step 2: Run** `cargo test -p reclip-core --test api` — Expected: FAIL.
- [ ] **Step 3: Implement** avec `axum::response::sse` et `tokio_stream::wrappers::BroadcastStream`.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): job events, cancel and delete routes`

---

### Task 6: Reprise au démarrage et nettoyage

**Files:**
- Create: `crates/core/src/jobs/{recovery,cleanup}.rs`
- Modify: `crates/core/src/main.rs`, `crates/core/src/jobs/mod.rs`, `crates/core/src/bin/fake-ytdlp.rs` (écrit ses arguments dans `<dossier de -o>/args.txt`)
- Test: `crates/core/tests/recovery.rs`

**Interfaces:**
- Consumes: Tasks 1 et 4.
- Produces:
  - `async fn recover(manager: &JobManager) -> anyhow::Result<RecoveryReport>` : `mark_interrupted` puis suppression des dossiers de ces tâches ; remise en file de `store.queued()` (avec propriétaire et `format_id` d'origine) ; suppression des dossiers de `downloads_dir` qui ne correspondent à aucune tâche `queued`/`done`. `struct RecoveryReport { interrupted: usize, requeued: usize, orphan_dirs: usize }`, loggé au démarrage.
  - `fn spawn_cleanup(manager: JobManager) -> JoinHandle<()>` : toutes les `cleanup_interval`, `expire_due(now)` → suppression des dossiers et publication de l'état `expired` ; `purge_expired(now − 7 jours)`.
  - `main.rs` appelle `recover` avant d'écouter et lance le nettoyage.

- [ ] **Step 1: Write the failing tests** : `recover_fails_running_and_requeues_queued` (store préparé : une tâche `downloading` avec dossier, une `queued` avec `format_id` `137` → après `recover` : la première `failed`/`interrupted` sans dossier, la seconde va jusqu'à `done` et le faux binaire a reçu `-f 137+bestaudio/best` — le faux binaire écrit ses arguments dans `job_dir/args.txt`), `recover_removes_orphan_dirs`, `cleanup_expires_and_deletes_files` (`file_ttl` 1 s, `cleanup_interval` 200 ms : une tâche `done` devient `expired`, fichier supprimé, `GET /file` → 404).
- [ ] **Step 2: Run** `cargo test -p reclip-core --test recovery` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): recover jobs on start and expire old files`

---

### Task 7: Frontend : progression, annulation, reprise

**Files:**
- Create: `web/src/progress.ts`, `web/src/storage.ts`, `web/src/ui/progress-bar.ts`, `web/tests/{progress,storage,restore}.test.ts`
- Modify: `web/src/api/client.ts`, `web/src/state.ts`, `web/src/ui/card.ts`, `web/src/main.ts`, `web/styles/cards.css`

**Interfaces:**
- Consumes: routes des Tasks 2 et 5, types régénérés en Task 1.
- Produces:
  - `client.ts` : `listJobs(): Promise<Job[]>`, `cancelJob(id): Promise<Job>`, `deleteJob(id): Promise<void>`, `eventsUrl(id): string`. Tous les `fetch` en `credentials: "same-origin"`.
  - `progress.ts` : `followJob(id, onJob: (job: Job) => void, deps = { EventSource, getJob }): () => void` : `EventSource` sur `eventsUrl(id)` ; à la fermeture après un état terminal, s'arrête ; en cas d'erreur réseau, retombe sur `pollJob` (1500 ms) ; renvoie une fonction d'arrêt.
  - `storage.ts` : `loadDraft(): string`, `saveDraft(text: string): void` (clé `reclip:draft`, chaque accès dans `try/catch`, renvoie `""` en cas d'échec).
  - `state.ts` : `cardFromJob(job: Job): Card` (statut `downloading` pour `queued`/`downloading`/`processing`, `done`, `error` pour `failed`/`cancelled` avec `jobError`) ; `Card` gagne `progress?: number`, `speed?: number`, `eta?: number`, `jobState?: JobState`.
  - `progress-bar.ts` : barre fine dans la couleur d'accent, texte DM Mono « 42 % · 3.1 MB/s · 0:12 » (« Queued » en file, « Processing » en traitement).
  - Carte : bouton **Cancel** pendant `queued`/`downloading`/`processing`, **Remove** une fois terminée ou en erreur (appelle `deleteJob` puis retire la carte).
  - Au chargement : `listJobs()` → une carte par tâche non expirée, ordre de la liste, suivi des tâches non terminées ; brouillon du champ de liens restauré, sauvegardé à chaque saisie, vidé après « Fetch ».

- [ ] **Step 1: Write the failing tests** : `progress.test.ts` (`followJob` avec un faux `EventSource` : livre chaque `job`, s'arrête au terminal ; sur `onerror` bascule sur `getJob` jusqu'au terminal), `storage.test.ts` (aller-retour ; `localStorage` qui lève → `""` sans exception), `restore.test.ts` (`cardFromJob` pour `queued`, `done`, `failed` avec code, `cancelled`), et un test du formatage de la barre (`formatProgress({progress: 42.4, speed: 3_250_000, eta: 12})` → `"42 % · 3.1 MB/s · 0:12"`).
- [ ] **Step 2: Run** `cd web && bun test` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `bun test && bun run typecheck && bun run build` — Expected: PASS.
- [ ] **Step 5: Vérifier dans le navigateur** (serveur avec faux yt-dlp, `RECLIP_MAX_CONCURRENT=1`) : coller `https://slow.example/1` et `https://ok`, télécharger tout ; la barre avance, la seconde carte affiche « Queued » ; rafraîchir : les deux cartes reviennent et la progression reprend ; Cancel arrête la première ; Remove retire la carte.
- [ ] **Step 6: Commit** `feat(web): live progress, cancel, remove and restore on refresh`

---

### Task 8: Tests de bout en bout (Playwright) et CI

**Files:**
- Create: `web/e2e/playwright.config.ts`, `web/e2e/downloads.spec.ts`
- Modify: `web/package.json` (dev dep `@playwright/test`, script `e2e`), `web/bun.lock`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: binaires `reclip-core` et `fake-ytdlp` (debug), `web/dist`.
- Produces: `webServer` Playwright qui lance `../target/debug/reclip-core` (extension `.exe` sous Windows) avec `RECLIP_YTDLP_BIN` = faux binaire, `RECLIP_PORT=18999`, `RECLIP_DATA_DIR` = dossier temporaire, `RECLIP_MAX_CONCURRENT=1`, `RECLIP_WEB_DIR=dist` ; Chromium uniquement.

- [ ] **Step 1: Write the specs** : `paste, fetch and download mp3` (la carte atteint « Save »), `progress survives a refresh` (lancer `https://slow.example/1`, attendre une progression > 0, recharger, la carte revient avec une progression), `cancel a running download` (texte « Download cancelled »), `remove a finished download` (la carte disparaît et ne revient pas après rechargement).
- [ ] **Step 2: Run** `cargo build -p reclip-core && cd web && bun run build && bunx playwright install chromium && bun run e2e` — Expected: PASS.
- [ ] **Step 3: CI** : job `e2e` (dépend de `rust` et `web`) : construit en debug, `bun install --frozen-lockfile`, `bun run build`, `bunx playwright install --with-deps chromium`, `bun run e2e` ; publie le rapport Playwright en artefact en cas d'échec.
- [ ] **Step 4: Commit** `test(e2e): Playwright downloads flow in CI`

---

### Task 9: Configuration et documentation

**Files:**
- Modify: `deploy/compose.yaml`, `deploy/.env.example`, `deploy/README.md`, `README.md`

- [ ] **Step 1:** Ajouter `RECLIP_MAX_CONCURRENT`, `RECLIP_MAX_PER_SITE`, `RECLIP_FILE_TTL`, `RECLIP_CLEANUP_INTERVAL` avec leurs défauts dans le compose et `.env.example` (commentaires en français), et au tableau du README (anglais). README : mentionner annulation, progression, reprise après rafraîchissement ; noter que l'API rattache les tâches au cookie `reclip_client` (un script `curl` doit garder ses cookies : `-c`/`-b`).
- [ ] **Step 2: Run** `docker compose -f deploy/compose.yaml config --quiet` et vérifier que chaque variable de `crates/core/src/config.rs` figure dans `.env.example` (`grep`).
- [ ] **Step 3: Commit** `docs: phase 2 settings`
