# Phase 3 : Accès et limites — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** L'API devient utilisable par des agents IA avec des clés révocables, chaque demandeur a des limites généreuses mais réelles, le serveur connaît la vraie IP derrière le proxy, refuse les cibles internes (SSRF) et les requêtes d'autres sites (Origin), envoie des en-têtes de sécurité stricts, et peut produire des liens de téléchargement signés.

**Architecture:** Un module `access` dans `reclip-core` regroupe tout le contrôle d'accès : vraie IP (`ip.rs`), clés (`keys.rs`), demandeur (`caller.rs`, middleware qui remplace `http/identity.rs`), limites (`limits.rs`, compteurs en mémoire), SSRF (`ssrf.rs`, résolveur injectable), secrets et adresse publique (`secrets.rs`). Les handlers extraient un `Caller` (propriétaire, clé de quota, limites, droits) au lieu d'un `Owner`. Les clés et réglages vivent dans SQLite (migration `0004`). Les routes admin et `/d/{jeton}` sont hors du middleware demandeur.

**Tech Stack:** Rust (axum 0.8 `ConnectInfo`, `middleware::from_fn_with_state`), `sha2` + `hmac` (empreintes et signatures), `ipnet` (plages CIDR), `tower-http` `set-header`, sqlx sqlite, TypeScript + Bun, Playwright.

**Spec:** `docs/superpowers/specs/2026-11-11-reclip-platform-design.md` (sections 3.1, 3.4, 3.5, 4, 7.2, 8 ; phase 3 de la section 9). Suites de la phase 2 : `docs/superpowers/notes/2026-11-12-phase2-followups.md`.

## Global Constraints

- Aucun fichier source au-delà de ~300 lignes.
- Types d'API uniquement dans `crates/shared` ; `web/src/api/types.gen.ts` régénéré (`UPDATE_TS=1 cargo test -p reclip-shared --test export_ts`), jamais édité à la main.
- Clé API : `rck_<id>_<secret>`, `id` = 8 caractères `[0-9a-z]`, `secret` = 43 caractères base62 (≥ 256 bits, tirage par rejet sur `getrandom`). Seul le SHA-256 hexadécimal du secret est stocké ; comparaison en temps constant.
- Droits (forme sur le fil) : `probe`, `jobs:create`, `jobs:read`, `jobs:cancel`, `delegate`. Une clé créée sans liste reçoit tous les droits sauf `delegate`. Un visiteur a les quatre premiers.
- Propriétaires : visiteur `web:<cookie>`, clé `key:<id>`, délégation `tg:<user_id>`. Clés de quota : visiteur `ip:<adresse>`, clé `key:<id>`, délégation `tg:<user_id>`.
- Limites par défaut (0 = illimité), réglables par variables d'environnement :

  | Niveau | Analyses / min | Tâches créées / heure | Tâches actives |
  |---|---|---|---|
  | `web` (par IP) | 120 | 100 | 80 |
  | `key` (par clé) | 120 | 200 | 100 |
  | `tg` (par utilisateur) | 30 | 40 | 10 |
  | clé `unlimited` | — | — | — |

  Variables : `RECLIP_LIMIT_{WEB,KEY,TG}_{PROBES_PER_MIN,JOBS_PER_HOUR,ACTIVE}`.
- Limite dépassée : `429`, code `rate_limited`, en-têtes `Retry-After`, `RateLimit-Limit`, `RateLimit-Remaining`, `RateLimit-Reset` (secondes). Les réponses acceptées des routes limitées portent aussi les trois `RateLimit-*`.
- Nouveaux codes d'erreur : `unauthorized` (401, clé absente, invalide, révoquée ou expirée ; en-tête `WWW-Authenticate: Bearer`) et `forbidden` (403, droit manquant, `Origin` refusée, délégation interdite).
- Proxys de confiance par défaut : `127.0.0.0/8,::1/128,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,fc00::/7` (`RECLIP_TRUSTED_PROXIES`). `X-Forwarded-For`, `X-Forwarded-Proto` et `X-Forwarded-Host` ne sont lus que si le pair TCP est de confiance.
- Une tâche d'un autre propriétaire répond toujours `404`.
- Une tâche n'est jamais refusée parce que d'autres tournent : seules les limites ci-dessus refusent.
- Aucun secret (clé, jeton admin, clé de signature) dans les logs, sauf le jeton admin généré, affiché une seule fois au démarrage qui le crée.
- Apparence du site inchangée.
- Commits terminés par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Lot de 12 liens puis une playlist de 50 depuis le navigateur** : aucun refus (visiteur : 100 créations par heure, 80 actives). → test `visitor_batch_of_62_is_accepted` dans Task 6.
2. **Requête directe au conteneur sans proxy avec un `X-Forwarded-For` forgé** : l'en-tête est ignoré, l'IP est celle du pair. → test `spoofed_forwarded_for_from_untrusted_peer_is_ignored` dans Task 2.
3. **Lien vers `http://169.254.169.254/`, `http://[::ffff:127.0.0.1]/`, `http://localhost:8899/` ou un nom qui résout vers `10.x`** : `403 forbidden_target`, yt-dlp jamais lancé. → tests dans Task 7.
4. **Clé révoquée ou expirée encore utilisée par un agent** : `401` immédiat, ses tâches restent invisibles aux autres. → test dans Task 5.
5. **Le site réel dans un navigateur avec la CSP active** : aucune violation CSP en console, barre de progression et miniatures visibles. → test Playwright dans Task 10.

---

## File Structure

| Fichier | Responsabilité |
|---|---|
| `crates/shared/src/{errors,api}.rs`, `crates/shared/src/admin.rs` | Codes `unauthorized`/`forbidden`, `PublicConfig` enrichi, `LinkResponse`, types admin des clés. |
| `crates/core/migrations/0004_access.sql` | Tables `api_keys`, `settings` ; colonne `jobs.quota_key`. |
| `crates/core/src/store/{keys,settings}.rs` | Requêtes clés et réglages. |
| `crates/core/src/access/{mod,ip,keys,limits,caller,quota,ssrf,secrets}.rs` | Contrôle d'accès. |
| `crates/core/src/http/{admin,links,headers}.rs` | Routes admin, liens signés, en-têtes de sécurité. |
| `crates/core/src/cli.rs` | Sous-commande `keys create|list|revoke`. |
| `crates/core/tests/common/mod.rs` | Serveur de test commun (pair TCP, résolveur fixe, `Origin`). |
| `web/src/{errors,api/client}.ts`, `web/src/ui/{card,progress-bar}.ts`, `web/styles/*.css` | Messages, balisage sans attribut `style`. |
| `deploy/{compose.yaml,.env.example,README.md}`, `README.md` | Variables et documentation. |

---

### Task 1: Types partagés, migration et stockage des clés

**Files:**
- Create: `crates/shared/src/admin.rs`, `crates/core/migrations/0004_access.sql`, `crates/core/src/store/keys.rs`, `crates/core/src/store/settings.rs`
- Modify: `crates/shared/src/{lib,errors,api}.rs`, `crates/shared/tests/export_ts.rs`, `web/src/api/types.gen.ts` (régénéré), `crates/core/src/store/{mod,queries,tests}.rs`, `crates/core/src/jobs/mod.rs` (appel `insert`)

**Interfaces:**
- Produces:
  - `ErrorCode::Unauthorized` (401) et `ErrorCode::Forbidden` (403).
  - `enum Scope { Probe, JobsCreate, JobsRead, JobsCancel, Delegate }`, sérialisé `"probe"`, `"jobs:create"`, `"jobs:read"`, `"jobs:cancel"`, `"delegate"` ; `Scope::default_set() -> Vec<Scope>` (tous sauf `Delegate`).
  - `struct KeyLimits { probes_per_minute: Option<u32>, jobs_per_hour: Option<u32>, max_active: Option<u32> }` (tous `#[serde(default)]`).
  - `struct CreateKeyRequest { name: String, scopes: Option<Vec<Scope>>, unlimited: bool (default false), expires_in_days: Option<u32>, limits: Option<KeyLimits> }`.
  - `struct ApiKeyInfo { id, name, scopes: Vec<Scope>, unlimited: bool, limits: KeyLimits, created_at, expires_at: Option<String>, revoked_at: Option<String>, last_used_at: Option<String>, request_count: u64 }` ; `struct CreatedKey { key: String, info: ApiKeyInfo }` ; `struct ApiKeyList { keys: Vec<ApiKeyInfo> }`.
  - `struct LinkResponse { url: String, expires_at: String }`.
  - `struct PublicLimits { probes_per_minute: u32, jobs_per_hour: u32, max_active: u32 }` ; `PublicConfig` gagne `public_web: bool`, `limits: PublicLimits` (niveau visiteur), `max_duration: u64`.
  - Store : `insert(&self, job: &Job, owner: &str, quota_key: &str)` ; `count_active(&self, quota_key: &str) -> Result<u32>` (états `queued`, `downloading`, `processing`) ; `insert_key(&self, info: &ApiKeyInfo, secret_sha256: &str)` ; `key_with_hash(&self, id: &str) -> Result<Option<(ApiKeyInfo, String)>>` ; `list_keys(&self) -> Result<Vec<ApiKeyInfo>>` (plus récentes d'abord) ; `revoke_key(&self, id: &str) -> Result<bool>` ; `touch_key(&self, id: &str) -> Result<()>` (`last_used_at = now`, `request_count + 1`) ; `setting(&self, k: &str) -> Result<Option<String>>` ; `set_setting(&self, k: &str, v: &str) -> Result<()>` ; `set_setting_if_absent(&self, k: &str, v: &str) -> Result<bool>`.

- [ ] **Step 1: Write the failing tests**. `crates/shared/tests/serde.rs` : `scopes_use_colon_names` (`Scope::JobsCreate` ↔ `"jobs:create"`), `create_key_request_minimal` (`{"name":"agent"}` se désérialise, `unlimited == false`), `new_error_codes_status` (401, 403). `store/tests.rs` : `count_active_by_quota_key` (2 `queued` + 1 `downloading` + 1 `done` sous `ip:1.2.3.4` → 3 ; autre clé → 0), `key_roundtrip_and_revoke` (insertion, `key_with_hash` renvoie l'empreinte, `revoke_key` → `revoked_at` posé, `revoke_key` d'un id inconnu → `false`), `touch_key_counts`, `settings_set_if_absent` (deuxième appel → `false`, valeur inchangée), `legacy_failed_rows_get_expiry` (ligne `failed` sans `expires_at` insérée avant migration 0004 → `expires_at` non nul après).
- [ ] **Step 2: Run** `cargo test -p reclip-shared && cargo test -p reclip-core store::` — Expected: FAIL.
- [ ] **Step 3: Implement**. Migration `0004` : table `api_keys(id TEXT PRIMARY KEY, name TEXT NOT NULL, secret_sha256 TEXT NOT NULL, scopes TEXT NOT NULL /* séparés par des virgules */, unlimited INTEGER NOT NULL DEFAULT 0, probes_per_minute INTEGER, jobs_per_hour INTEGER, max_active INTEGER, created_at TEXT NOT NULL, expires_at TEXT, revoked_at TEXT, last_used_at TEXT, request_count INTEGER NOT NULL DEFAULT 0)` ; table `settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)` ; `ALTER TABLE jobs ADD COLUMN quota_key TEXT NOT NULL DEFAULT ''` ; index `(quota_key, state)` ; `UPDATE jobs SET expires_at = <now> WHERE state IN ('failed','cancelled') AND expires_at IS NULL` (suite de la phase 2). Pour le test de rattrapage, appliquer les migrations 0001 à 0003 à la main dans une base mémoire, insérer la ligne, puis 0004 (ou exposer un `Store::open_until` réservé aux tests, au choix). Appelants actuels de `insert` : clé de quota `""` en attendant la Task 6. Ajouter les nouveaux types à `export_ts.rs`, régénérer.
- [ ] **Step 4: Run** `cargo test --workspace && cd web && bun run typecheck` — Expected: PASS.
- [ ] **Step 5: Commit** `feat: api key, settings and quota storage`

---

### Task 2: Configuration, proxys de confiance et vraie IP

**Files:**
- Create: `crates/core/src/access/mod.rs`, `crates/core/src/access/ip.rs`
- Modify: `crates/core/Cargo.toml` (`ipnet = "2"`), `crates/core/src/{lib,config}.rs`

**Interfaces:**
- Produces:
  - `Config` gagne : `trusted_proxies: Vec<ipnet::IpNet>`, `public_url: Option<String>` (`RECLIP_PUBLIC_URL`, sans `/` final), `admin_token: Option<String>` (`RECLIP_ADMIN_TOKEN`), `public_web: bool` (`RECLIP_PUBLIC_WEB`, défaut `true` ; `false`, `0`, `no` → `false`), `link_ttl: Duration` (`RECLIP_LINK_TTL`, défaut 21600 s), `limits: TierLimits`.
  - `struct Limits { probes_per_minute: u32, jobs_per_hour: u32, max_active: u32 }` (0 = illimité) ; `struct TierLimits { web: Limits, key: Limits, tg: Limits }` avec les défauts de la table des contraintes.
  - Une entrée invalide de `RECLIP_TRUSTED_PROXIES` est ignorée avec un avertissement ; une adresse seule vaut `/32` ou `/128`.
  - `access::ip::Forwarded { client_ip: IpAddr, trusted_peer: bool, https: bool, host: Option<String> }` ; `fn forwarded(peer: Option<IpAddr>, headers: &HeaderMap, trusted: &[IpNet]) -> Forwarded`. Pair absent → `0.0.0.0`, non fiable. Pair fiable : parcourir `X-Forwarded-For` de droite à gauche en sautant les adresses de confiance, prendre la première qui ne l'est pas (toutes de confiance → la plus à gauche ; en-tête absent ou illisible → le pair) ; `https` = `X-Forwarded-Proto` vaut `https` ; `host` = `X-Forwarded-Host` sinon `Host`. Pair non fiable : IP = pair, `https = false`, `host` = `Host`. Les adresses IPv4 mappées en IPv6 sont ramenées en IPv4.

- [ ] **Step 1: Write the failing tests** (`config.rs`, `access/ip.rs`) : `access_defaults` (proxys par défaut = 6 plages, `public_web`, `link_ttl` 21600, limites de la table), `limits_override_and_zero` (`RECLIP_LIMIT_TG_ACTIVE=0` → 0), `bad_proxy_entry_skipped`, `public_web_false_values`, `trusted_peer_uses_rightmost_untrusted` (pair `10.0.1.9`, XFF `203.0.113.7, 10.0.0.3` → `203.0.113.7`), `spoofed_forwarded_for_from_untrusted_peer_is_ignored` (pair `198.51.100.2`, XFF `1.1.1.1`, `X-Forwarded-Proto: https` → IP `198.51.100.2`, `https == false`), `mapped_ipv6_normalized`, `missing_peer_is_untrusted`.
- [ ] **Step 2: Run** `cargo test -p reclip-core config:: access::ip` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test -p reclip-core` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): trusted proxies and client ip`

---

### Task 3: Clés API : format, empreinte et vérification

**Files:**
- Create: `crates/core/src/access/keys.rs`
- Modify: `crates/core/Cargo.toml` (`sha2 = "0.10"`, `hmac = "0.12"`), `crates/core/src/access/mod.rs`

**Interfaces:**
- Consumes: Task 1 (store, types admin).
- Produces:
  - `fn random_base62(len: usize) -> anyhow::Result<String>` ; `fn sha256_hex(s: &str) -> String` ; `fn ct_eq(a: &[u8], b: &[u8]) -> bool`.
  - `async fn create_key(store: &Store, req: CreateKeyRequest) -> anyhow::Result<CreatedKey>` : `id` 8 caractères `[0-9a-z]` (nouveau tirage si déjà pris), `expires_at = now + expires_in_days`, nom vide ou > 64 caractères → erreur `invalid_request`.
  - `fn parse_key(raw: &str) -> Option<(&str, &str)>` : `rck_<id>_<secret>` exact, sinon `None`.
  - `enum KeyError { Invalid }` ; `async fn verify_key(store: &Store, bearer: &str) -> Result<ApiKeyInfo, KeyError>` : clé mal formée, inconnue, empreinte différente, révoquée ou expirée → `Invalid` (même réponse pour tous les cas) ; succès → `touch_key` lancé en tâche de fond.

- [ ] **Step 1: Write the failing tests** : `key_format` (préfixe `rck_`, 8 + 43 caractères, alphabet respecté), `parse_rejects_malformed` (`rck_abc_x`, `rck_ABCDEFGH_…`, `Bearer rck_…`, chaîne vide), `verify_ok_then_revoked`, `verify_expired` (`expires_at` passé), `verify_wrong_secret`, `stored_hash_is_not_secret` (la colonne ne contient pas le secret), `ct_eq_basic`.
- [ ] **Step 2: Run** `cargo test -p reclip-core access::keys` — Expected: FAIL.
- [ ] **Step 3: Implement**. Base62 par rejet : octets `>= 248` ignorés, puis `% 62`.
- [ ] **Step 4: Run** `cargo test -p reclip-core` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): api key format and verification`

---

### Task 4: Limiteur en mémoire

**Files:**
- Create: `crates/core/src/access/limits.rs`

**Interfaces:**
- Produces:
  - `enum Bucket { Probe, Create }` (fenêtres 60 s et 3600 s).
  - `struct RateInfo { limit: u32, remaining: u32, reset_secs: u64 }` : `impl IntoResponseParts` (pose `RateLimit-Limit`, `RateLimit-Remaining`, `RateLimit-Reset`).
  - `struct Limited { info: RateInfo, retry_after: u64, message: String }` : `impl IntoResponse` (429, JSON `{code: "rate_limited", message}`, `Retry-After` + `RateLimit-*`).
  - `struct Limiter` (`Clone`, `Arc<Mutex<HashMap<(Bucket, String), Window>>>`) : `fn check(&self, bucket: Bucket, key: &str, limit: u32) -> Result<Option<RateInfo>, Limited>` (limite 0 → `Ok(None)` sans compter) et `fn check_at(&self, …, now: Instant)` pour les tests. Fenêtre fixe : premier appel ouvre la fenêtre, refus quand `count == limit`, `reset_secs` = temps restant arrondi au-dessus. Message : `Too many requests, try again in {n} s`. Toutes les 1024 vérifications, les fenêtres échues sont retirées.

- [ ] **Step 1: Write the failing tests** : `allows_up_to_limit_then_429` (limite 3 : trois `Ok` avec `remaining` 2, 1, 0, puis `Err` avec `retry_after > 0`), `window_resets` (`check_at` + 61 s → `Ok`), `keys_are_independent`, `zero_is_unlimited`, `limited_response_has_headers` (statut 429, quatre en-têtes, corps JSON).
- [ ] **Step 2: Run** `cargo test -p reclip-core access::limits` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test -p reclip-core` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): in-memory rate limiter`

---

### Task 5: Demandeur : cookie, clé, délégation, Origin et droits

**Files:**
- Create: `crates/core/src/access/caller.rs`, `crates/core/tests/common/mod.rs`, `crates/core/tests/access.rs`
- Delete: `crates/core/src/http/identity.rs` (logique du cookie déplacée dans `caller.rs`)
- Modify: `crates/core/src/app.rs`, `crates/core/src/main.rs`, `crates/core/src/http/{mod,jobs,files,events,probe}.rs`, `crates/core/tests/{api,api_events,jobs,recovery}.rs` (passent par `common`)

**Interfaces:**
- Consumes: Tasks 2, 3, 4.
- Produces:
  - `AppState` gagne `limiter: Limiter`.
  - `enum CallerKind { Visitor, Key, Delegated }` ; `struct Caller { owner: String, quota_key: String, kind: CallerKind, scopes: Vec<Scope>, limits: Option<Limits> /* None = unlimited */ , key_id: Option<String> }` ; `fn require(&self, s: Scope) -> Result<(), HttpError>` (→ 403 `forbidden`). Extracteur `FromRequestParts` comme l'ancien `Owner`.
  - `caller_layer` (`middleware::from_fn_with_state`) sur `/api/v1/*` :
    1. `Forwarded` à partir de `ConnectInfo<SocketAddr>` (absent → `None`).
    2. `Authorization: Bearer …` présent → `verify_key` (échec → 401 `unauthorized` + `WWW-Authenticate: Bearer`, message `Invalid or expired API key`). Limites = celles de la clé (champs `None` → niveau `key`), `unlimited` → `None`.
    3. Avec clé, `X-Reclip-On-Behalf-Of` présent : exige `delegate` (sinon 403), valeur `tg:` + 1 à 20 chiffres (sinon 400 `invalid_request`) ; propriétaire et clé de quota = cette valeur, limites niveau `tg` (même pour une clé `unlimited`), droits = ceux de la clé moins `delegate`.
    4. Sans clé : si `public_web == false` et route autre que `GET /api/v1/config` → 401. Sinon cookie comme en phase 2 (création si absent, `Secure` si `Forwarded.https`), propriétaire `web:<cookie>`, quota `ip:<client_ip>`, limites `web`.
    5. Visiteur et méthode `POST`/`PUT`/`PATCH`/`DELETE` : `Origin` obligatoire, son autorité (`hôte[:port]`) doit égaler `Forwarded.host` ou l'autorité de l'adresse publique connue ; sinon 403 `forbidden`, message `Cross-site request refused`.
    6. Réponse : `Cache-Control: no-store`, `Set-Cookie` si cookie créé (jamais pour une clé).
  - Droits par route : `probe` → `Probe` ; `POST /jobs` → `JobsCreate` ; `GET /jobs`, `GET /jobs/{id}`, `/events`, `/file` → `JobsRead` ; `cancel`, `DELETE` → `JobsCancel`.
  - `app::serve` sert `into_make_service_with_connect_info::<SocketAddr>()`.
  - `tests/common/mod.rs` : `TestServer::start(tweak: impl FnOnce(&mut Config))` (dossiers temporaires, faux yt-dlp, `ConnectInfo`) ; `Visitor` (cookies, en-tête `Origin` = adresse du serveur par défaut) ; `Agent { key }` (en-tête `Authorization`) ; `wait_job`. Les quatre fichiers de tests existants l'utilisent ; leurs assertions ne changent pas.

- [ ] **Step 1: Write the failing tests** (`tests/access.rs`) : `visitor_without_origin_cannot_post` (POST `/jobs` sans `Origin` → 403 `forbidden` ; avec `Origin: https://evil.example` → 403), `visitor_get_needs_no_origin`, `key_owns_its_jobs` (clé A crée une tâche, la liste ; clé B et un visiteur ont 404 dessus), `bad_key_401` (inconnue, mal formée, révoquée, expirée → 401 + `WWW-Authenticate`), `key_scope_enforced` (clé avec `["jobs:read"]` : POST `/jobs` → 403, GET `/jobs` → 200), `delegation_requires_scope` (clé sans `delegate` + en-tête → 403 ; avec → la tâche a le propriétaire `tg:42`, invisible pour `tg:43`), `delegation_header_validated` (`tg:abc` → 400), `private_mode_requires_key` (`public_web = false` : visiteur POST `/probe` → 401, `GET /config` → 200, clé → 200), `key_requests_set_no_cookie`.
- [ ] **Step 2: Run** `cargo test -p reclip-core --test access` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS (anciens tests inclus).
- [ ] **Step 5: Commit** `feat(core): api keys, delegation, origin check and scopes`

---

### Task 6: Quotas sur l'analyse et la création

**Files:**
- Create: `crates/core/src/access/quota.rs`
- Modify: `crates/core/src/http/{probe,jobs}.rs`, `crates/core/src/jobs/mod.rs` (`create` reçoit la clé de quota), `crates/core/tests/access.rs`

**Interfaces:**
- Consumes: Tasks 4, 5.
- Produces:
  - `async fn check_probe(s: &AppState, c: &Caller) -> Result<Option<RateInfo>, Limited>` : `Bucket::Probe`, clé `c.quota_key`.
  - `async fn check_create(s: &AppState, c: &Caller) -> Result<Option<RateInfo>, Response>` : d'abord le plafond de tâches actives (`store.count_active(quota_key) >= max_active` → `Limited` avec `retry_after = 30`, message `Too many downloads in progress (max {n})`), puis `Bucket::Create`. Erreur store → 500.
  - `JobManager::create(&self, owner: &str, quota_key: &str, req)`.
  - `probe` et `POST /jobs` posent `RateInfo` sur leur réponse réussie.

- [ ] **Step 1: Write the failing tests** : `visitor_batch_of_62_is_accepted` (limites par défaut, 62 créations `https://slow.example/n` depuis la même IP → toutes 202), `active_cap` (`limits.web.max_active = 2` avec des tâches lentes : la 3e → 429 `rate_limited`, `Retry-After: 30`), `probe_limit_headers` (`limits.web.probes_per_minute = 2` : deux 200 avec `RateLimit-Remaining` 1 puis 0, troisième 429 avec `Retry-After`), `create_hourly_limit` (`jobs_per_hour = 2`), `unlimited_key_never_limited` (clé `unlimited` + limites 1 : 3 analyses passent, aucun en-tête `RateLimit-*`), `tg_limits_apply_to_delegation` (`limits.tg.max_active = 1` : 2e création déléguée → 429), `quota_shared_by_cookies_of_same_ip` (deux visiteurs, même IP, `jobs_per_hour = 1` → le second reçoit 429).
- [ ] **Step 2: Run** `cargo test -p reclip-core --test access` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): per-requester quotas with RateLimit headers`

---

### Task 7: Protection SSRF

**Files:**
- Create: `crates/core/src/access/ssrf.rs`
- Modify: `crates/core/src/app.rs` (`AppState.resolver`), `crates/core/src/http/{mod,probe,jobs}.rs`, `crates/core/tests/common/mod.rs` (résolveur fixe), `crates/core/tests/access.rs`

**Interfaces:**
- Produces:
  - `type Resolver = Arc<dyn Fn(String, u16) -> BoxFuture<'static, std::io::Result<Vec<IpAddr>>> + Send + Sync>` ; `fn system_resolver() -> Resolver` (`tokio::net::lookup_host`) ; `fn static_resolver(ip: IpAddr) -> Resolver`.
  - `fn is_forbidden(ip: IpAddr) -> bool` : IPv4 `0.0.0.0/8`, `10/8`, `100.64/10`, `127/8`, `169.254/16`, `172.16/12`, `192.0.0/24`, `192.168/16`, `198.18/15`, `224/4`, `240/4` (dont `255.255.255.255`) ; IPv6 `::`, `::1`, `fc00::/7`, `fe80::/10`, `ff00::/8`, `64:ff9b::/96`, et toute IPv4 mappée (`::ffff:a.b.c.d`) testée comme IPv4.
  - `async fn check_target(url: &str, resolver: &Resolver) -> Result<(), ApiError>` : hôte `localhost` ou `*.localhost` → refus ; IP littérale → `is_forbidden` ; nom → résolution (délai 5 s, port par défaut du schéma), une seule adresse interdite suffit à refuser. Refus → `forbidden_target`, message `This address is not allowed`. Échec ou délai de résolution → `invalid_request`, message `Cannot resolve this link's host`.
  - `probe` et `POST /jobs` appellent `check_target` après `validate_url`, avant les quotas de création (une cible refusée ne consomme pas de quota).

- [ ] **Step 1: Write the failing tests** : unitaires `forbidden_ranges` (chaque plage ci-dessus, plus `93.184.216.34` et `2606:4700::1` autorisés), `mapped_loopback_forbidden`, `localhost_names`, `one_bad_address_is_enough` (résolveur qui renvoie une adresse publique et `10.0.0.1`) ; intégration `metadata_url_rejected` (`http://169.254.169.254/latest` → 403 `forbidden_target`, aucune tâche créée), `name_resolving_private_rejected` (résolveur fixe `10.1.2.3`).
- [ ] **Step 2: Run** `cargo test -p reclip-core access::ssrf --test access` — Expected: FAIL.
- [ ] **Step 3: Implement**. `common` installe `static_resolver(93.184.216.34)` pour que les URL de test (`https://ok`, `https://slow.example/…`) passent.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): reject private and metadata targets`

---

### Task 8: Secrets, adresse publique, admin et commande `keys`

**Files:**
- Create: `crates/core/src/access/secrets.rs`, `crates/core/src/http/admin.rs`, `crates/core/src/cli.rs`
- Modify: `crates/core/src/{app,main}.rs`, `crates/core/src/access/caller.rs` (déduction de l'adresse), `crates/core/src/http/config.rs`, `crates/core/tests/access.rs`

**Interfaces:**
- Consumes: Tasks 1, 3, 5.
- Produces:
  - `struct Site { public_url: Arc<RwLock<Option<String>>>, admin_hash: String, link_key: [u8; 32] }` dans `AppState` ; `async fn Site::load(cfg: &Config, store: &Store) -> anyhow::Result<Site>` :
    - jeton admin : `RECLIP_ADMIN_TOKEN` s'il est défini (empreinte calculée, rien de stocké) ; sinon réglage `admin_token_sha256` ; sinon génération `rca_` + 43 base62, empreinte stockée, jeton affiché une fois en `WARN` (`admin token (shown once, store it now): …`) ;
    - clé de signature : réglage `link_key` (64 hex) ou génération ;
    - adresse publique : `RECLIP_PUBLIC_URL`, sinon réglage `public_url`, sinon `None`.
  - `fn Site::learn(&self, store: &Store, fwd: &Forwarded)` : appelée par `caller_layer` ; si aucune adresse connue, pair de confiance avec `X-Forwarded-Proto` et `X-Forwarded-Host` → `{proto}://{host}`, `set_setting_if_absent("public_url", …)`, log `INFO` une fois.
  - Routes admin (hors `caller_layer`, en-tête `Authorization: Bearer <jeton admin>`, comparaison en temps constant des empreintes, échec → 401) : `POST /api/v1/admin/keys` → 201 `CreatedKey` ; `GET /api/v1/admin/keys` → `ApiKeyList` ; `DELETE /api/v1/admin/keys/{id}` → 204 ou 404.
  - `cli::run(args: &[String]) -> anyhow::Result<Option<i32>>` appelé par `main` avant le serveur : `keys create --name <n> [--scopes a,b] [--unlimited] [--expires-days N]` (affiche la clé complète une fois), `keys list` (une ligne par clé, jamais de secret), `keys revoke <id>` ; `None` si les arguments ne sont pas une sous-commande `keys`.
  - `GET /api/v1/config` renvoie `public_web`, `limits` (niveau `web`), `max_duration`.

- [ ] **Step 1: Write the failing tests** : unitaires `admin_token_generated_once` (deux `Site::load` sur la même base → même empreinte, réglage présent), `env_admin_token_not_stored`, `link_key_persisted`, `public_url_learned_from_trusted_proxy_only` (pair non fiable → rien) ; `cli.rs` : `parses_create_flags`, `unknown_subcommand_is_none` ; intégration `admin_key_lifecycle` (création → la clé fonctionne → liste sans secret → révocation → 401), `admin_requires_token` (sans jeton, mauvais jeton, jeton visiteur → 401), `config_exposes_limits`.
- [ ] **Step 2: Run** `cargo test -p reclip-core` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): admin token, key management and public url`

---

### Task 9: Liens signés

**Files:**
- Create: `crates/core/src/http/links.rs`
- Modify: `crates/core/src/app.rs`, `crates/core/src/http/files.rs` (service du fichier partagé), `crates/core/tests/access.rs`

**Interfaces:**
- Consumes: Task 8 (`Site.link_key`, adresse publique).
- Produces:
  - `fn sign(key: &[u8; 32], id: &str, exp: i64) -> String` → `"{id}.{exp}.{sig}"`, `sig` = 32 premiers caractères hexadécimaux de HMAC-SHA256(`"{id}.{exp}"`) ; `fn verify(key, token: &str, now: i64) -> Option<String>` (renvoie l'id ; signature comparée en temps constant ; expiré ou mal formé → `None`).
  - `POST /api/v1/jobs/{id}/link` (droit `JobsRead`, tâche du demandeur à l'état `done`, sinon 404) → `LinkResponse` ; expiration = min(`now + link_ttl`, `expires_at` de la tâche) ; adresse = `{public_url}/d/{jeton}`, ou à défaut l'origine de la requête (`Forwarded.https` + `Forwarded.host`).
  - `GET /d/{token}` (hors `caller_layer`, sans cookie) : jeton valide et tâche `done` → fichier avec `Range` et `Content-Disposition` comme `/file` ; sinon 404 `Link expired or invalid`.
  - `files.rs` expose `async fn serve_job_file(s: &AppState, job: Job, req: Request) -> Response`, utilisé par les deux routes.

- [ ] **Step 1: Write the failing tests** : unitaires `sign_verify_roundtrip`, `tampered_token_rejected` (id, date ou signature modifiés), `expired_token_rejected` ; intégration `link_downloads_without_cookie` (client sans cookies, 200, `Range: bytes=0-3` → 206), `link_for_unfinished_job_404`, `link_of_other_owner_404`, `link_capped_by_file_expiry`, `link_dead_after_job_deleted`.
- [ ] **Step 2: Run** `cargo test -p reclip-core` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): signed download links`

---

### Task 10: En-têtes de sécurité et frontend compatible CSP

**Files:**
- Create: `crates/core/src/http/headers.rs`
- Modify: `crates/core/Cargo.toml` (`tower-http` feature `set-header`), `crates/core/src/app.rs`, `web/src/ui/{card,progress-bar}.ts`, `web/styles/*.css`, `web/src/{errors,api/client}.ts`, `web/tests/{errors,client}.test.ts`, `web/e2e/downloads.e2e.ts`

**Interfaces:**
- Produces:
  - Sur toutes les réponses (site, API, `/d`, `/healthz`) : `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data: https:; connect-src 'self'; media-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=(), usb=()`.
  - Plus aucun attribut `style="…"` dans le HTML généré par le frontend (la CSP les bloque) : couleurs par classes CSS (`.no-thumb`, `.card-title.error`), largeur de la barre posée par `element.style.width` après insertion (autorisé par la CSP).
  - `ApiFailure` gagne `retryAfter?: number` (lu dans `Retry-After`). `friendlyError` : `rate_limited` renvoie le message du serveur ; `unauthorized` → `This server requires an API key`, `forbidden` → `This action was refused`, `forbidden_target` → `This link points to a private address`.

- [ ] **Step 1: Write the failing tests** : Rust `security_headers_on_all_routes` (index, `/api/v1/config`, 404 d'API, `/healthz`) ; Bun `retry_after_is_parsed`, textes des trois nouveaux codes, `progress_markup_has_no_style_attribute` ; Playwright : le test existant écoute `console` et `pageerror` et échoue sur tout message contenant `Content Security Policy` ; il vérifie aussi que la barre de progression a une largeur non nulle pendant un téléchargement lent.
- [ ] **Step 2: Run** `cargo test -p reclip-core && cd web && bun test` — Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test --workspace && cd web && bun run typecheck && bun test && bun run build && bunx playwright test -c e2e/playwright.config.ts` — Expected: PASS.
- [ ] **Step 5: Commit** `feat: strict security headers and csp-safe markup`

---

### Task 11: Déploiement et documentation

**Files:**
- Modify: `deploy/compose.yaml`, `deploy/.env.example`, `deploy/README.md` (français), `README.md` (anglais)

**Interfaces:**
- Produces:
  - Compose : `RECLIP_PUBLIC_URL`, `RECLIP_TRUSTED_PROXIES`, `RECLIP_ADMIN_TOKEN`, `RECLIP_PUBLIC_WEB` (`true`), `RECLIP_LINK_TTL` (`21600`) et les neuf `RECLIP_LIMIT_*` avec leurs défauts `${VAR:-…}`.
  - `.env.example` : chaque variable commentée en français.
  - `deploy/README.md` : sections « Jeton admin » (où le trouver dans les logs, comment le fixer), « Clés API » (`docker compose exec core reclip-core keys create --name agent`, puis `curl` avec `Authorization: Bearer`), « Limites », « Adresse publique et proxy ».
  - `README.md` : section « API for agents » : création de clé, exemples `curl` (`probe`, `jobs`, suivi, fichier), codes d'erreur, en-têtes `RateLimit-*`.

- [ ] **Step 1: Run** `docker compose -f deploy/compose.yaml config` après modification — Expected: OK, toutes les nouvelles variables présentes avec leur défaut.
- [ ] **Step 2: Vérifier** que chaque variable lue par `Config::from_env` figure dans `compose.yaml` et `.env.example` (`grep -o 'RECLIP_[A-Z_]*' crates/core/src/config.rs | sort -u` comparé aux deux fichiers).
- [ ] **Step 3: Commit** `docs: api keys, limits and proxy settings`
