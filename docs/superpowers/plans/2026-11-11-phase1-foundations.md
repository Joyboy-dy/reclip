# Phase 1 : Fondations — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remplacer le backend Flask et la page monolithique par un workspace Rust (`shared`, `core`) et un frontend TypeScript compilé par Bun, à parité fonctionnelle avec l'existant, déployable via `deploy/compose.yaml`.

**Architecture:** `core` (axum + tokio) expose `/api/v1`, lance le binaire yt-dlp en sous-processus, garde les tâches dans SQLite et sert `web/dist`. `shared` porte les types de l'API et les exporte en TypeScript avec `ts-rs`. Un faux yt-dlp (binaire Rust du workspace) rend tous les tests indépendants du réseau.

**Tech Stack:** Rust stable (édition 2024), axum 0.8, tokio 1, serde/serde_json, sqlx 0.8 (sqlite, runtime-tokio, migrate, sans macros vérifiées à la compilation), uuid (v7), thiserror, tracing, ts-rs, tower-http (fs, trace), reqwest (dev, tests). Bun 1.3.x, TypeScript 5.x. Docker, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-11-reclip-platform-design.md` (sections 2, 3.1, 3.2, 3.4, 3.5, 5.1, 7, 8, et la phase 1 de la section 9).

## Global Constraints

- Aucun fichier source au-delà de ~300 lignes.
- Les types de l'API vivent dans `crates/shared` ; `web/src/api/types.gen.ts` est généré, jamais édité à la main.
- Toutes les routes sous `/api/v1` ; les anciennes routes `/api/*` disparaissent.
- Port par défaut 8899.
- Runtime JS : Deno 2.5.6 dans l'image (`COPY --from=denoland/deno:bin-2.5.6 /deno /usr/local/bin/deno`) ; détection Deno → Bun → Node hors Docker ; `RECLIP_JS_RUNTIME` force le choix.
- Cookies : `RECLIP_COOKIES` copié dans un fichier temporaire par appel, supprimé après ; fichier absent = ignoré.
- Proxy : `RECLIP_PROXY` → `--proxy`.
- Défauts : `RECLIP_JOB_TIMEOUT=1800` s, `RECLIP_PLAYLIST_LIMIT=50`, `RECLIP_MAX_FILESIZE=2G`, `RECLIP_MAX_DURATION=10800` s.
- Codes d'erreur (snake_case en JSON) : `bot_check`, `unsupported_url`, `unavailable`, `private`, `geo_blocked`, `too_large`, `too_long`, `timeout`, `interrupted`, `cancelled`, `rate_limited`, `forbidden_target`, `invalid_request`, `internal`.
- Apparence du site inchangée : Instrument Serif, DM Mono, accent `#e85d2a`.
- Image `core` < 350 Mo, utilisateur non-root, `HEALTHCHECK` sur `/healthz`.
- Commits terminés par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Lien collé avec espaces, texte autour ou schéma non http(s)** (`ftp://`, `javascript:`) : le serveur répond `400 invalid_request`, il ne lance pas yt-dlp. → test dans Task 7.
2. **Titre avec emoji, `/`, `\`, guillemets ou 300 caractères** : nom de fichier sûr (≤ 100 caractères, sans séparateur) et `Content-Disposition` en `filename*=UTF-8''…`. → tests dans Task 4 et Task 7.
3. **yt-dlp laisse plusieurs fichiers** (`.webm`, `.m4a`, `.mp4`) : on garde le `.mp4` (ou `.mp3`) et on supprime le reste. → test dans Task 5.
4. **Binaire yt-dlp introuvable** : la tâche échoue avec `internal` et le message « yt-dlp not found », sans panique du serveur. → test dans Task 5.
5. **Sortie JSON précédée de lignes non-JSON ou contenant plusieurs objets** : on prend le premier objet valide. → test dans Task 4.

---

## File Structure

| Fichier | Responsabilité |
|---|---|
| `Cargo.toml` | Workspace (`crates/*`), profil release (`lto = "thin"`, `strip = true`). |
| `crates/shared/src/lib.rs` | Ré-exports. |
| `crates/shared/src/api.rs` | `ProbeRequest`, `ProbeResponse`, `VideoInfo`, `PlaylistInfo`, `FormatOption`, `CreateJobRequest`, `Job`, `JobState`, `MediaFormat`, `PublicConfig`. |
| `crates/shared/src/errors.rs` | `ErrorCode`, `ApiError`. |
| `crates/shared/tests/export_ts.rs` | Génère et vérifie `web/src/api/types.gen.ts`. |
| `crates/core/src/main.rs` | Démarrage. |
| `crates/core/src/config.rs` | `Config::from_env`. |
| `crates/core/src/ytdlp/args.rs` | Arguments communs et par opération. |
| `crates/core/src/ytdlp/parse.rs` | Lecture du JSON, qualités, nom de fichier. |
| `crates/core/src/ytdlp/errors.rs` | Classement de stderr en `ErrorCode` + message. |
| `crates/core/src/ytdlp/runner.rs` | Lancement, délai, choix du fichier final. |
| `crates/core/src/store/mod.rs` + `migrations/` | SQLite : tâches. |
| `crates/core/src/http/{mod,probe,jobs,files,config}.rs` | Routes. |
| `crates/core/src/jobs.rs` | Création et exécution d'une tâche (sans file : phase 2). |
| `crates/core/src/bin/fake-ytdlp.rs` | Faux yt-dlp pour les tests. |
| `web/…` | Frontend (Task 8). |
| `deploy/…` | Déploiement (Task 9). |
| `.github/workflows/ci.yml` | CI (Task 10). |

---

### Task 1: Workspace et types partagés

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml` (channel `stable`), `crates/shared/Cargo.toml`, `crates/shared/src/{lib,api,errors}.rs`
- Test: `crates/shared/tests/serde.rs`

**Interfaces:**
- Produces:
  - `enum MediaFormat { Mp4, Mp3 }` (serde `lowercase`)
  - `enum JobState { Queued, Downloading, Processing, Done, Cancelled, Failed, Expired }` (serde `snake_case`)
  - `enum ErrorCode { BotCheck, UnsupportedUrl, Unavailable, Private, GeoBlocked, TooLarge, TooLong, Timeout, Interrupted, Cancelled, RateLimited, ForbiddenTarget, InvalidRequest, Internal }` (serde `snake_case`), avec `fn http_status(&self) -> u16` (`InvalidRequest`/`UnsupportedUrl` → 400, `ForbiddenTarget` → 403, `RateLimited` → 429, `Internal` → 500, autres → 422)
  - `struct ApiError { code: ErrorCode, message: String }`
  - `struct FormatOption { id: String, label: String, height: u32 }`
  - `struct VideoInfo { title, thumbnail, duration: Option<f64>, uploader, formats: Vec<FormatOption> }`
  - `struct PlaylistInfo { title: String, entries: Vec<String> }` (URLs)
  - `enum ProbeResponse { Video(VideoInfo), Playlist(PlaylistInfo) }` (serde `tag = "kind"`, `rename_all = "lowercase"`)
  - `struct ProbeRequest { url: String }`, `struct CreateJobRequest { url: String, format: MediaFormat, format_id: Option<String>, title: Option<String> }`
  - `struct Job { id: String, url: String, format: MediaFormat, title: Option<String>, state: JobState, filename: Option<String>, size: Option<u64>, error: Option<ApiError>, created_at: String }` (RFC 3339)
  - `struct PublicConfig { version: String, telegram_bot_username: Option<String>, playlist_limit: u32 }`
  - Tous dérivent `Serialize, Deserialize, Debug, Clone, PartialEq` et `ts_rs::TS`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn error_code_is_snake_case() {
    assert_eq!(serde_json::to_string(&ErrorCode::BotCheck).unwrap(), "\"bot_check\"");
}
#[test] fn probe_response_is_tagged_by_kind() {
    let v = serde_json::to_value(ProbeResponse::Playlist(PlaylistInfo { title: "t".into(), entries: vec![] })).unwrap();
    assert_eq!(v["kind"], "playlist");
}
#[test] fn job_state_round_trips() {
    for s in [JobState::Queued, JobState::Done, JobState::Failed] {
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<JobState>(&j).unwrap(), s);
    }
}
#[test] fn http_status_mapping() {
    assert_eq!(ErrorCode::InvalidRequest.http_status(), 400);
    assert_eq!(ErrorCode::RateLimited.http_status(), 429);
    assert_eq!(ErrorCode::BotCheck.http_status(), 422);
}
```

- [ ] **Step 2: Run** `cargo test -p reclip-shared` — Expected: FAIL (types absents).
- [ ] **Step 3: Implement** les types ci-dessus dans `api.rs` et `errors.rs`.
- [ ] **Step 4: Run** `cargo test -p reclip-shared` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(shared): API types and error codes`

---

### Task 2: Export TypeScript des types

**Files:**
- Create: `crates/shared/tests/export_ts.rs`, `web/src/api/types.gen.ts` (généré)

**Interfaces:**
- Consumes: types de Task 1.
- Produces: `web/src/api/types.gen.ts`, qui exporte les mêmes noms que Task 1.

- [ ] **Step 1: Write the test** `types_gen_is_up_to_date` : concatène `T::decl()` (via `ts_rs::TS`) de tous les types de Task 1, précédé de l'en-tête `// Generated by crates/shared/tests/export_ts.rs. Do not edit.` ; si la variable `UPDATE_TS=1` est définie, écrit le fichier ; sinon compare au fichier existant et échoue avec « run UPDATE_TS=1 cargo test -p reclip-shared » en cas d'écart.
- [ ] **Step 2: Run** `cargo test -p reclip-shared --test export_ts` — Expected: FAIL (fichier absent).
- [ ] **Step 3: Run** `UPDATE_TS=1 cargo test -p reclip-shared --test export_ts`, puis sans la variable — Expected: PASS.
- [ ] **Step 4: Commit** `feat(shared): generate TypeScript types`

---

### Task 3: Configuration et arguments yt-dlp

**Files:**
- Create: `crates/core/Cargo.toml` (bin `reclip-core`, bin `fake-ytdlp`), `crates/core/src/{main,config}.rs`, `crates/core/src/ytdlp/{mod,args}.rs`
- Test: tests unitaires dans `config.rs` et `args.rs`

**Interfaces:**
- Produces:
  - `struct Config { host: String, port: u16, data_dir: PathBuf, downloads_dir: PathBuf, web_dir: PathBuf, ytdlp_bin: String, cookies: Option<PathBuf>, proxy: Option<String>, js_runtime: Option<String>, job_timeout: Duration, playlist_limit: u32, max_filesize: String, max_duration: u64, telegram_bot_username: Option<String> }`
  - `fn Config::from_env(env: &dyn Fn(&str) -> Option<String>) -> Config`. Défauts : `host` `0.0.0.0`, `port` 8899 (`PORT` ou `RECLIP_PORT`), `data_dir` `./data`, `downloads_dir` `<data_dir>/downloads`, `web_dir` `./web/dist`, `ytdlp_bin` `yt-dlp` (`RECLIP_YTDLP_BIN`), plus les défauts des Global Constraints. Variables vides = absentes.
  - `fn js_runtime_args(forced: Option<&str>, which: &dyn Fn(&str) -> bool) -> Vec<String>`
  - `struct SessionArgs { pub args: Vec<String>, _cookies: Option<tempfile::TempPath> }` et `fn session_args(cfg: &Config) -> std::io::Result<SessionArgs>` (le fichier temporaire disparaît au `drop`)
  - `fn probe_args(url: &str, playlist_limit: u32) -> Vec<String>` : `["--no-playlist", "-j", url]` ; si l'URL contient `list=` : `["--flat-playlist", "-J", "--playlist-end", "<limit>", url]`
  - `fn download_args(out_template: &str, format: MediaFormat, format_id: Option<&str>, max_filesize: &str, max_duration: u64, url: &str) -> Vec<String>`

- [ ] **Step 1: Write the failing tests** (portage des 12 tests Python + nouveaux)

```rust
#[test] fn deno_needs_no_flag() { assert!(js_runtime_args(None, &|n| n == "deno" || n == "bun").is_empty()); }
#[test] fn bun_then_node_fallback() {
    assert_eq!(js_runtime_args(None, &|n| n == "bun"), ["--js-runtimes", "bun"]);
    assert_eq!(js_runtime_args(None, &|n| n == "node"), ["--js-runtimes", "node"]);
}
#[test] fn forced_runtime_wins() { assert_eq!(js_runtime_args(Some("quickjs:/opt/qjs"), &|_| true), ["--js-runtimes", "quickjs:/opt/qjs"]); }
#[test] fn cookies_are_copied_and_removed() { /* écrit cookies.txt dans un tempdir, cfg.cookies = Some(path);
    let s = session_args(&cfg)?; args contient "--cookies" suivi d'un chemin != original au même contenu;
    drop(s); ce chemin n'existe plus; l'original existe toujours */ }
#[test] fn missing_cookies_file_is_ignored() { /* cfg.cookies pointe vers un fichier absent → args sans "--cookies" */ }
#[test] fn proxy_is_passed() { /* cfg.proxy = Some("socks5://127.0.0.1:7897") → ["--proxy","socks5://127.0.0.1:7897"] dans args */ }
#[test] fn playlist_urls_are_capped() {
    assert_eq!(probe_args("https://youtube.com/watch?v=a&list=RDx", 50), ["--flat-playlist", "-J", "--playlist-end", "50", "https://youtube.com/watch?v=a&list=RDx"]);
}
#[test] fn mp3_download_args() {
    let a = download_args("/d/x.%(ext)s", MediaFormat::Mp3, None, "2G", 10800, "u");
    assert!(a.windows(2).any(|w| w == ["--audio-format", "mp3"]) && a.contains(&"-x".into()));
}
#[test] fn mp4_with_format_id() {
    let a = download_args("/d/x.%(ext)s", MediaFormat::Mp4, Some("137"), "2G", 10800, "u");
    assert!(a.windows(2).any(|w| w == ["-f", "137+bestaudio/best"]));
    assert!(a.windows(2).any(|w| w == ["--merge-output-format", "mp4"]));
    assert!(a.windows(2).any(|w| w == ["--max-filesize", "2G"]));
    assert!(a.windows(2).any(|w| w == ["--match-filter", "duration <=? 10800"]));
}
#[test] fn config_defaults_and_overrides() {
    let c = Config::from_env(&|_| None);
    assert_eq!((c.port, c.playlist_limit, c.job_timeout.as_secs()), (8899, 50, 1800));
    let c = Config::from_env(&|k| (k == "RECLIP_PORT").then(|| "9000".into()));
    assert_eq!(c.port, 9000);
    let c = Config::from_env(&|k| (k == "RECLIP_PROXY").then(|| "".into()));
    assert_eq!(c.proxy, None);
}
```

- [ ] **Step 2: Run** `cargo test -p reclip-core` — Expected: FAIL.
- [ ] **Step 3: Implement** `config.rs` et `args.rs`. Les arguments de téléchargement commencent toujours par `["--no-playlist", "--newline", "-o", out_template]`. `main.rs` lit la config et logge « config loaded » (serveur en Task 7).
- [ ] **Step 4: Run** `cargo test -p reclip-core` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): config and yt-dlp arguments`

---

### Task 4: Lecture de la sortie yt-dlp et classement des erreurs

**Files:**
- Create: `crates/core/src/ytdlp/{parse,errors}.rs`

**Interfaces:**
- Produces:
  - `fn parse_first_json(stdout: &str) -> Result<serde_json::Value, ApiError>` (ignore lignes vides et non-JSON ; aucune → `Internal` « yt-dlp returned no data »)
  - `fn video_info(v: &Value) -> VideoInfo` (meilleur format par hauteur selon `tbr`, seulement `vcodec != "none"`, tri par hauteur décroissante, `label = "{h}p"`)
  - `fn playlist_info(v: &Value) -> PlaylistInfo` (URLs des `entries[*].url` non vides)
  - `fn safe_filename(title: &str, ext: &str, fallback: &str) -> String` (retire `\ / : * ? " < > |` et les caractères de contrôle, coupe à 100 caractères (pas octets), `trim` ; vide → `fallback`)
  - `fn classify(stderr: &str, cookies_configured: bool) -> ApiError`

Table de classement (sur la dernière ligne commençant par `ERROR:`, sinon la dernière ligne non vide, insensible à la casse) :

| Contient | Code |
|---|---|
| `not a bot` | `bot_check` (+ aide cookies : « set RECLIP_COOKIES… » ou « …rejected or have expired… » selon `cookies_configured`, textes identiques à `ytdlp_options.py`) |
| `unsupported url` | `unsupported_url` |
| `private video`, `login required`, `requires authentication` | `private` |
| `not available in your country`, `geo` + `restrict` | `geo_blocked` |
| `video unavailable`, `has been removed`, `404` | `unavailable` |
| `larger than max-filesize`, `file is larger than` | `too_large` |
| `does not pass filter` + `duration` | `too_long` |
| sinon | `internal` (message = la ligne) ; stderr vide → « yt-dlp failed without an error message » |

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn first_json_skips_noise_and_extra_objects() {
    let v = parse_first_json("WARNING: x\n\n{\"id\":\"a\"}\n{\"id\":\"b\"}\n").unwrap();
    assert_eq!(v["id"], "a");
}
#[test] fn best_format_per_height() { /* 2 formats 720p (tbr 1000 et 2500) + 1 audio-only → 1 option "720p" d'id du tbr 2500 */ }
#[test] fn safe_filename_strips_and_truncates() {
    assert_eq!(safe_filename("a/b\\c:d*e?\"f<g>h|i", "mp4", "x"), "abcdefghi.mp4");
    assert_eq!(safe_filename(&"é".repeat(300), "mp3", "x").chars().count(), 100 + 4);
    assert_eq!(safe_filename("  ", "mp4", "job123"), "job123.mp4");
    assert_eq!(safe_filename("🎬 clip", "mp4", "x"), "🎬 clip.mp4");
}
#[test] fn classifies_bot_check_with_hint() {
    let e = classify("ERROR: [youtube] a: Sign in to confirm you’re not a bot.", false);
    assert_eq!(e.code, ErrorCode::BotCheck);
    assert!(e.message.contains("RECLIP_COOKIES"));
    assert!(classify("ERROR: … not a bot …", true).message.contains("expired"));
}
#[test] fn classifies_common_errors() {
    assert_eq!(classify("ERROR: Unsupported URL: https://x", false).code, ErrorCode::UnsupportedUrl);
    assert_eq!(classify("ERROR: [youtube] a: Video unavailable", false).code, ErrorCode::Unavailable);
    assert_eq!(classify("", false).message, "yt-dlp failed without an error message");
}
```

- [ ] **Step 2: Run** `cargo test -p reclip-core ytdlp::` — Expected: FAIL.
- [ ] **Step 3: Implement** `parse.rs` et `errors.rs`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): parse yt-dlp output and classify errors`

---

### Task 5: Faux yt-dlp et exécution

**Files:**
- Create: `crates/core/src/bin/fake-ytdlp.rs`, `crates/core/src/ytdlp/runner.rs`
- Test: `crates/core/tests/runner.rs`

**Interfaces:**
- Consumes: Task 3 (`session_args`, `probe_args`, `download_args`), Task 4 (`parse_first_json`, `video_info`, `playlist_info`, `classify`).
- Produces:
  - `async fn probe(cfg: &Config, url: &str) -> Result<ProbeResponse, ApiError>` (délai 60 s → `timeout`)
  - `async fn download(cfg: &Config, job_dir: &Path, format: MediaFormat, format_id: Option<&str>, url: &str) -> Result<PathBuf, ApiError>` : lance yt-dlp avec `-o <job_dir>/media.%(ext)s`, délai `cfg.job_timeout` (dépassé → tue le processus, `timeout`), puis garde le fichier `.mp4` (ou `.mp3`), sinon le premier fichier, et supprime les autres. Binaire introuvable (`ErrorKind::NotFound`) → `internal` « yt-dlp not found ». Aucun fichier → `internal` « download finished but no file was found ».
  - Le faux binaire, piloté par l'URL (dernier argument) :
    - contient `error-bot` → stderr `ERROR: [youtube] x: Sign in to confirm you're not a bot.`, code 1
    - contient `slow` → dort 30 s
    - contient `multi-files` → écrit `media.webm`, `media.m4a`, `media.mp4`
    - `-j` → stdout `WARNING: noise` puis un JSON vidéo (`title` « Fake ✨ / video », 2 formats 720p, 1 format 1080p, `duration` 12)
    - `-J` → JSON playlist de 3 entrées
    - `-o <template>` → écrit `template` avec `%(ext)s` remplacé par `mp3` si `-x` est présent, sinon `mp4`, contenu `fake media`

- [ ] **Step 1: Write the failing tests** (le binaire via `env!("CARGO_BIN_EXE_fake-ytdlp")`, `Config` de test avec `ytdlp_bin` = ce chemin et `job_timeout` = 2 s)

```rust
#[tokio::test] async fn probe_video() { let ProbeResponse::Video(v) = probe(&cfg, "https://ok").await.unwrap() else { panic!() };
    assert_eq!(v.formats.iter().map(|f| f.height).collect::<Vec<_>>(), [1080, 720]); }
#[tokio::test] async fn probe_playlist() { /* "https://ok?list=PL1" → Playlist avec 3 entries */ }
#[tokio::test] async fn probe_bot_check() { assert_eq!(probe(&cfg, "https://error-bot").await.unwrap_err().code, ErrorCode::BotCheck); }
#[tokio::test] async fn download_keeps_only_mp4() { let p = download(&cfg, dir, MediaFormat::Mp4, None, "https://multi-files").await.unwrap();
    assert_eq!(p.extension().unwrap(), "mp4"); assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1); }
#[tokio::test] async fn download_mp3() { /* extension mp3 */ }
#[tokio::test] async fn download_times_out() { assert_eq!(download(&cfg, dir, MediaFormat::Mp4, None, "https://slow").await.unwrap_err().code, ErrorCode::Timeout); }
#[tokio::test] async fn missing_binary() { /* ytdlp_bin = "definitely-not-yt-dlp" → Internal, message "yt-dlp not found" */ }
```

- [ ] **Step 2: Run** `cargo test -p reclip-core --test runner` — Expected: FAIL.
- [ ] **Step 3: Implement** `fake-ytdlp.rs` puis `runner.rs` (`tokio::process::Command`, `kill_on_drop(true)`, `tokio::time::timeout`).
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): run yt-dlp with timeout, test double`

---

### Task 6: Stockage SQLite des tâches

**Files:**
- Create: `crates/core/migrations/0001_jobs.sql`, `crates/core/src/store/mod.rs`

**Interfaces:**
- Produces:
  - `struct Store(SqlitePool)` ; `async fn Store::open(path: &Path) -> anyhow::Result<Store>` (crée le dossier, mode WAL, applique `sqlx::migrate!()`) ; `async fn Store::memory() -> Store` (tests)
  - `async fn insert(&self, job: &Job) -> Result<()>`, `async fn get(&self, id: &str) -> Result<Option<Job>>`, `async fn set_state(&self, id: &str, state: JobState, filename: Option<&str>, size: Option<u64>, error: Option<&ApiError>) -> Result<()>`, `async fn file_path(&self, id: &str) -> Result<Option<PathBuf>>`, `async fn set_file_path(&self, id: &str, path: &Path) -> Result<()>`
  - Table `jobs` : `id TEXT PRIMARY KEY, url, format, format_id, title, state, filename, size INTEGER, file_path, error_code, error_message, created_at, finished_at, expires_at`. Index sur `state`.

- [ ] **Step 1: Write the failing tests**: `insert_then_get_round_trips` (tous les champs de `Job`), `set_state_records_error` (`Failed` + `ApiError{BotCheck, "m"}` relu à l'identique), `get_unknown_is_none`.
- [ ] **Step 2: Run** `cargo test -p reclip-core store::` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): SQLite job store`

---

### Task 7: API HTTP, exécution des tâches et fichiers statiques

**Files:**
- Create: `crates/core/src/http/{mod,probe,jobs,files,config}.rs`, `crates/core/src/jobs.rs`, `crates/core/src/app.rs`
- Modify: `crates/core/src/main.rs`
- Test: `crates/core/tests/api.rs`

**Interfaces:**
- Consumes: Tasks 3 à 6.
- Produces:
  - `struct AppState { cfg: Arc<Config>, store: Store }` ; `fn router(state: AppState) -> axum::Router` dans `app.rs`
  - `fn validate_url(raw: &str) -> Result<String, ApiError>` : `trim`, doit être une URL `http` ou `https` analysable (crate `url`), sinon `invalid_request` « Please paste a valid http(s) link ». Le contrôle SSRF arrive en phase 3.
  - `async fn spawn_job(state: AppState, req: CreateJobRequest) -> Result<Job, ApiError>` dans `jobs.rs` : `id` UUID v7 (forme simple), insère `Queued`, lance une tâche tokio qui passe à `Downloading`, appelle `runner::download` dans `downloads_dir/<id>/`, puis `Done` (avec `filename = safe_filename(title, ext, id)` et `size`) ou `Failed`.
  - Routes : `GET /healthz` → `200 ok` ; `GET /api/v1/config` → `PublicConfig` ; `POST /api/v1/probe` ; `POST /api/v1/jobs` → `202 Job` ; `GET /api/v1/jobs/{id}` → `Job` ou `404` ; `GET /api/v1/jobs/{id}/file` → fichier (`tower_http::services::ServeFile` pour `Range`) avec `Content-Disposition: attachment; filename="<ascii>"; filename*=UTF-8''<percent-encodé>`, `404` si pas `Done`. Fallback : `ServeDir(web_dir)` avec `index.html`.
  - Erreurs : `ApiError` implémente `IntoResponse` avec `code.http_status()` et le JSON `{code, message}`.

- [ ] **Step 1: Write the failing tests** (serveur sur port 0, `reqwest`, faux yt-dlp, `Store::memory()`, `web_dir` = tempdir avec `index.html` = `<h1>ReClip</h1>`)

```rust
#[tokio::test] async fn rejects_bad_urls() {
    for u in ["", "  ", "ftp://x/y", "javascript:alert(1)", "hello"] {
        let r = post("/api/v1/probe", json!({"url": u})).await;
        assert_eq!(r.status(), 400); assert_eq!(r.json::<Value>().await["code"], "invalid_request");
    }
}
#[tokio::test] async fn probe_trims_and_returns_video() { /* url "  https://ok  " → 200, kind "video" */ }
#[tokio::test] async fn bot_check_is_422_with_code() { /* https://error-bot → 422, code "bot_check" */ }
#[tokio::test] async fn job_lifecycle_and_unicode_filename() {
    // POST /api/v1/jobs {url:"https://ok", format:"mp4", title:"Fake ✨ / video"} → 202
    // poll GET /api/v1/jobs/{id} jusqu'à state "done" (≤ 5 s)
    // filename == "Fake ✨  video.mp4"
    // GET /file → 200, corps "fake media", Content-Disposition contient "filename*=UTF-8''Fake%20%E2%9C%A8%20%20video.mp4"
}
#[tokio::test] async fn file_supports_range() { /* Range: bytes=0-3 → 206, corps "fake" */ }
#[tokio::test] async fn failed_job_reports_code() { /* https://error-bot → state "failed", error.code "bot_check" */ }
#[tokio::test] async fn unknown_job_is_404() {}
#[tokio::test] async fn serves_index_and_config() { /* GET / contient "ReClip" ; GET /api/v1/config → playlist_limit 50 */ }
```

- [ ] **Step 2: Run** `cargo test -p reclip-core --test api` — Expected: FAIL.
- [ ] **Step 3: Implement**. `main.rs` : `tracing_subscriber` (filtre `RUST_LOG`, défaut `info`), `Store::open(data_dir/reclip.db)`, crée `downloads_dir`, écoute `host:port`, arrêt propre sur SIGTERM/Ctrl-C. Logs d'accès via `tower_http::trace`.
- [ ] **Step 4: Run** `cargo test -p reclip-core` — Expected: PASS (toutes les suites).
- [ ] **Step 5: Commit** `feat(core): HTTP API v1, job execution, static web`

---

### Task 8: Frontend TypeScript

**Files:**
- Create: `web/package.json`, `web/tsconfig.json`, `web/build.ts`, `web/index.html`, `web/styles/{base,form,cards}.css`, `web/src/main.ts`, `web/src/state.ts`, `web/src/urls.ts`, `web/src/api/client.ts`, `web/src/ui/{input,format-toggle,card,footer}.ts`, `web/static/favicon.svg` (déplacé depuis `static/`), `web/tests/{urls,state,client}.test.ts`
- Source à porter : `templates/index.html` (CSS lignes 10-~420, JS ~420-705)

**Interfaces:**
- Consumes: `web/src/api/types.gen.ts` (Task 2), routes de Task 7.
- Produces:
  - `parseUrls(text: string): string[]` dans `urls.ts` (séparateurs espaces, virgules, retours à la ligne ; dédoublonnage en gardant l'ordre ; ne garde que ce qui commence par `http://` ou `https://`)
  - `client.ts` : `getConfig(): Promise<PublicConfig>`, `probe(url): Promise<ProbeResponse>`, `createJob(req: CreateJobRequest): Promise<Job>`, `getJob(id): Promise<Job>`, `fileUrl(id): string` ; une réponse non-2xx lève `ApiFailure { code: ErrorCode, message: string, status: number }`.
  - `state.ts` : `type Card = { url, status: "loading" | "ready" | "info-error" | "downloading" | "done" | "error", info?: VideoInfo, jobId?: string, error?: string, selectedFormat?: string }` ; `expandPlaylists(urls, probe)` remplace une URL de playlist par ses entrées ; `pollJob(id, getJob, onUpdate, intervalMs = 1500)`.
  - Scripts : `bun run build` (→ `web/dist/` : `index.html`, `app.js` minifié, `styles.css` concaténé, `favicon.svg`), `bun run dev` (build en mode watch), `bun test`, `bun run typecheck` (`tsc --noEmit`).

- [ ] **Step 1: Write the failing tests**

```ts
test("parseUrls splits, trims, dedupes and drops non-links", () => {
  expect(parseUrls("https://a , https://b\nhttps://a hello ftp://x")).toEqual(["https://a", "https://b"]);
});
test("expandPlaylists replaces a playlist by its entries", async () => {
  const probe = async (u: string) => u.includes("list=")
    ? { kind: "playlist", title: "p", entries: ["https://1", "https://2"] }
    : { kind: "video", title: "v", thumbnail: "", duration: 1, uploader: "", formats: [] };
  expect(await expandPlaylists(["https://x?list=1", "https://y"], probe)).toEqual(["https://1", "https://2", "https://y"]);
});
test("client throws ApiFailure with the server code", async () => {
  globalThis.fetch = async () => new Response(JSON.stringify({ code: "bot_check", message: "m" }), { status: 422 });
  await expect(probe("https://x")).rejects.toMatchObject({ code: "bot_check", status: 422 });
});
```

- [ ] **Step 2: Run** `cd web && bun test` — Expected: FAIL.
- [ ] **Step 3: Implement** en reprenant le CSS et le comportement de `templates/index.html` à l'identique (MP4/MP3, cartes, qualités, « Download » et « Download all »). `footer.ts` ajoute, si `telegram_bot_username` est défini, un lien `https://t.me/<username>` sous la liste des sites : icône Telegram (SVG inline récupéré sur SVGL, couleur courante), texte « Use on Telegram », `target="_blank" rel="noopener"`.
- [ ] **Step 4: Run** `bun test`, `bun run typecheck`, `bun run build` — Expected: PASS, `web/dist/index.html` présent.
- [ ] **Step 5: Vérifier dans le navigateur** : `cargo run -p reclip-core` avec `RECLIP_YTDLP_BIN` = faux binaire et `RECLIP_TELEGRAM_BOT_USERNAME=socialvideodl_bot`. Coller `https://ok`, télécharger en MP4 puis en MP3, vérifier l'apparence face à `assets/preview-mp3.png` et la présence du lien Telegram.
- [ ] **Step 6: Commit** `feat(web): TypeScript frontend built with Bun`

---

### Task 9: Déploiement

**Files:**
- Create: `deploy/core.Dockerfile`, `deploy/compose.yaml`, `deploy/.env.example`, `deploy/entrypoint.sh`, `deploy/README.md`, `.dockerignore` (racine, réécrit)
- Modify: `.gitignore` (`/target`, `web/node_modules`, `web/dist`, `data/`), `crates/core/src/main.rs` (sous-commande `healthcheck`)

**Interfaces:**
- Consumes: binaire `reclip-core`, `web/dist`.
- Produces: image `reclip-core` ; service compose `core`.

Contenu imposé :
- `core.Dockerfile`, contexte = racine du dépôt : étape `chef` (`lukemathwalker/cargo-chef`, image Rust stable épinglée) → `planner` → `builder` (`cargo build --release -p reclip-core --bin reclip-core`) ; étape `web` (`oven/bun:1.3.14`, `bun install --frozen-lockfile && bun run build`) ; étape finale `debian:bookworm-slim` avec `ca-certificates`, `yt-dlp_linux` de la dernière release téléchargé en `/usr/local/bin/yt-dlp`, `COPY --from=denoland/deno:bin-2.5.6 /deno /usr/local/bin/deno`, ffmpeg et ffprobe statiques depuis une image épinglée (`mwader/static-ffmpeg:<version>`), utilisateur `reclip` (uid 1000), `RECLIP_DATA_DIR=/data`, `RECLIP_WEB_DIR=/app/web`, `EXPOSE 8899`, `HEALTHCHECK CMD ["reclip-core","healthcheck"]` (sous-commande qui appelle `/healthz` en local et sort 0 ou 1 : l'image n'a pas `curl`).
- `entrypoint.sh` : `yt-dlp -U` sauf `RECLIP_NO_UPDATE` (le binaire doit être modifiable par l'utilisateur `reclip`), puis `exec reclip-core`.
- `compose.yaml` : service `core` (`build: { context: .., dockerfile: deploy/core.Dockerfile }`), ports `${RECLIP_PORT:-8899}:8899`, volume `data:/data`, réseau `core_net`, `restart: unless-stopped`, et tous les `RECLIP_*` de cette phase avec leurs défauts `${VAR:-…}`. Montage des cookies commenté, comme dans l'ancien `docker-compose.yml`.
- `.env.example` : chaque variable, une ligne de commentaire en français, valeur par défaut.
- `README.md` : lancement local (`docker compose -f deploy/compose.yaml up --build`), passage de Dokploy en type « Compose » (chemin `deploy/compose.yaml`, variables dans l'onglet Environment, domaine sur le port 8899), cookies YouTube (reprise de la section du README racine).

- [ ] **Step 1: Run** `docker compose -f deploy/compose.yaml config` — Expected: OK, sans avertissement.
- [ ] **Step 2: Run** `docker build -f deploy/core.Dockerfile -t reclip-core:dev .` puis `docker image ls reclip-core:dev` — Expected: build OK, taille < 350 Mo.
- [ ] **Step 3: Smoke test réel** : `docker compose -f deploy/compose.yaml up -d`, attendre `healthy`, `POST /api/v1/probe` sur `https://www.youtube.com/watch?v=ZR95IRL1HYY`, puis un job MP3 jusqu'à `done` et `GET /file` en 200. Vérifier dans les logs `JS runtimes: deno` (lancer une fois `docker compose exec core yt-dlp -v --skip-download <url>`).
- [ ] **Step 4: Commit** `feat(deploy): core image and compose`

---

### Task 10: CI

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: commandes des Tasks 1 à 9.

Jobs : `rust` (ubuntu-latest, `dtolnay/rust-toolchain@stable` avec `rustfmt, clippy`, `Swatinem/rust-cache`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`) ; `web` (`oven-sh/setup-bun` en 1.3.14, `bun install --frozen-lockfile`, `bun run typecheck`, `bun test`, `bun run build`) ; `docker` (dépend des deux : `docker compose -f deploy/compose.yaml config`, build de l'image, échec si > 350 Mo). Actions épinglées par tag de version majeure.

- [ ] **Step 1: Run** localement `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace` et les commandes `web` — Expected: tout passe.
- [ ] **Step 2: Commit** `ci: Rust, web and image checks`

---

### Task 11: Retrait du Python et documentation

**Files:**
- Delete: `app.py`, `ytdlp_options.py`, `templates/`, `static/`, `requirements.txt`, `pytest.ini`, `tests/`, `reclip.sh`, `Dockerfile`, `docker-compose.yml`, `docker-entrypoint.sh`, `venv/` (non suivi)
- Modify: `README.md`

- [ ] **Step 1: Supprimer** les fichiers et vérifier `git grep -nE "app\.py|reclip\.sh|templates/|/api/(info|download|status|file|playlist)"` — Expected: aucune occurrence hors `docs/`.
- [ ] **Step 2: README** : Quick Start via `deploy/`, tableau des variables (renvoie à `deploy/.env.example`), section « YouTube on a server » conservée, Stack mise à jour (Rust, TypeScript/Bun, yt-dlp, Deno, ffmpeg), mention de `cargo run -p reclip-core` + `bun run dev` pour développer.
- [ ] **Step 3: Run** la suite complète (Task 10, Step 1) — Expected: PASS.
- [ ] **Step 4: Commit** `chore: remove the Flask app`
