# Task 1 report: backend contracts (original + fix round 1)

Branch `worktree-agent-af1acddea11247172`, based on feat/phase3b-ui (55d1149). The worktree initially sat on main (Python app); I ran `git reset --hard feat/phase3b-ui` before any work.

## Original task (commit 77b67d7)
- shared: MediaFormat (8 variants, is_audio), Clip, FormatOption.codec, PlaylistEntry, CreateJobRequest clip/strip_metadata/playlist_item, Job clip/strip_metadata/resumable, PublicConfig file_ttl_secs/history_days, history.rs, AdminLogin/AdminMe/AdminPasswordChange; export_ts updated, types.gen.ts regenerated.
- config: concurrent_fragments(4), impersonate(chrome; set-but-empty disables), max_disk (5G; K/M/G parser), file_ttl 21600, history_days 30, admin_emails (lowercased), admin_default_password (Secret), admin_session_ttl 12h. argon2 0.5 and base64 0.22 added.
- migrations 0005, 0006. Store reads/writes the new columns, `resumable` computed, `insert_item` and `playlist_item(id)`; timestamp helpers moved to store/time.rs; empty store/{history,admin}.rs and access/admin_auth.rs.
- ytdlp: DownloadOptions, `download_args(out_dir, &opts, cfg, url)`, `runner::download_with`, `runner::download` kept as wrapper. Placeholder logic for new formats.
- http: pages.rs (nonce pages), history.rs / admin_session.rs / jobs::resume answering 501 "Not implemented yet"; headers.rs keeps an existing CSP.
- TDD: RED = `cargo test -p reclip-shared --test serde` failing with E0425/E0422 (HistoryClean, Clip, AdminLogin, AdminPasswordChange not found); GREEN = all shared tests, then config/store/pages/headers tests.
- Verification then: fmt and clippy -D warnings clean; cargo test --workspace all green (core lib 146; access 15, admin 7, api 15, api_events 9, headers 1, jobs 10, links 7, pages 4, quotas 9, recovery 4, runner 13; shared serde 13 + export_ts 1); bun test 45/45. `bun run typecheck` fails only in pre-existing web code/tests (owned by the web task).

## Fix round 1
1. `Clip.end` now `#[ts(optional = nullable)]` (TS `end?: number | null`); it was the only server-emitted type using plain `#[ts(optional)]`; the others (admin.rs, CreateJobRequest, HistoryClean) are request types. types.gen.ts regenerated with `UPDATE_TS=1 cargo test -p reclip-shared`.
2. app.rs: `/index.html`, `/admin.html` and `/admin/` now go through the nonce handlers; test extended (tests/pages.rs).
3. config.rs: invalid `RECLIP_MAX_DISK` logs a warning and falls back to 5G.
4. worker.rs: `store.playlist_item` is read before `create_dir_all` (and before set_state).

Commands: `UPDATE_TS=1 cargo test -p reclip-shared`; `cargo fmt --all`; `cargo clippy --workspace --all-targets -- -D warnings` (clean); `cargo test --workspace` (summary below).

Results: cargo test --workspace all ok (core lib 146, access 15, admin 7, api 15, api_events 9, headers 1, jobs 10, links 7, pages 4, quotas 9, recovery 4, runner 13, shared 13+1). One run under heavy machine load failed runner::cancel_stops_download (timing); re-run of that binary and of the whole workspace passed; runner code untouched by this round. fmt and clippy -D warnings clean. types.gen.ts: `end?: number | null`.
