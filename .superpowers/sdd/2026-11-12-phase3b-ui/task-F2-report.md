# Task F2 report: /admin React app

Status: DONE_WITH_CONCERNS. Branch worktree-agent-aaa3d36eb79f0c9c2 (reset to feat/phase3b-ui d035056), worktree
`.claude/worktrees/agent-aaa3d36eb79f0c9c2`, commit dd5198b `feat(web): admin app for api keys and password`.
(Report written inside the worktree: the shared-checkout path was refused.)

## Implemented (web/src/admin/, all files < 215 lines)
- main.tsx: session bootstrap (GET /me), routing gate: anonymous -> sign in; must_change_password -> first-login only; else keys/password.
- route.ts: History API router (/admin, /admin/keys, /admin/password; /admin lands on keys). breakpoint.ts: phone <= 560, tablet <= 900.
- api.ts: same-origin fetch client for session/me/password/keys (reuses ApiFailure). lib.ts: pure logic (daysLeft, expiryInfo with soon <= 7 days, keyStatus, rights/limits labels, relativeTime, password strength/validation, create-request builder).
- auth.tsx (login, first login), shell.tsx (desktop header, tablet header, phone top bar + DropdownMenu), keys-page.tsx, key-views.tsx (table with calendar Tooltip; 2-col tablet cards; phone cards with expiry written), new-key.tsx (Dialog desktop/tablet, Sheet phone; shadcn Checkbox/Select), key-created.tsx (locked: no Esc/outside close; copy button), revoke.tsx (radix AlertDialog; centered or bottom sheet), password-page.tsx, modal.tsx, parts.tsx.

## Verification
- `bun test tests/admin-lib.test.ts`: 9/9 pass. Full `bun test`: 58 pass, 1 fail (a followJob polling test in the public-page area, not touched by me; client.test.ts alone passes).
- `bunx tsc --noEmit -p .`: 0 errors in src/admin and tests/admin*; remaining errors are in the public page files (other agent).
- `bun run build`: OK (admin.js + app.css).
- Manual visual check in the browser pane against a mock API server (scratchpad): desktop login/keys/new key/created, tablet keys grid + revoke dialog, phone keys cards + new key sheet + password page matched mockups. Not visually checked: first-login screen, phone menu dropdown, phone created/revoke sheets, tooltip hover.
- TDD: lib tests written together with lib (no separate RED run recorded).

## Concerns
- Server must serve admin.html for /admin, /admin/keys, /admin/password (client-side routes).
- First login: the sign-in password is kept in memory and sent as `current` to POST /admin/password; after a reload the screen asks for the default password. Confirm backend expects `current` there.
- Key id is shown as `rck_<id>_…` (id assumed bare, per keys.rs).
- Revoke uses radix-ui AlertDialog directly (no shadcn alert-dialog in ui/); no ui files edited.
- Password errors: 401 on change shows "The current password is not correct."; unknown errors show the server message.

## Fix round 1
- 401: one path. api.ts `isUnauthorized` + `SESSION_EXPIRED`; keys list, create, revoke, password change and first-login all call `onUnauthorized` (main.tsx `expired`) which returns to sign-in showing "Your session expired. Sign in again." Login itself overrides 401 with "Wrong email or password.". Removed the password-page 401 special case.
- 429: api.ts parses Retry-After into ApiFailure.retryAfter; text "Too many attempts. Try again in N s." / fallback "...in a moment."
- Minor: single exported `capitalize` (lib.ts); logout failure caught, still signs out locally; sign-in password kept in state only while must_change is true; /admin replaced by /admin/keys when signed in.
- Tests: new tests/admin-api.test.ts (adminErrorText 401/429/other/network, parseRetryAfter, fetch Retry-After propagation, parseRoute/routePath, capitalize).
- Command: `bun test tests/admin-lib.test.ts tests/admin-api.test.ts` -> 16 pass, 0 fail; tsc: no errors in src/admin or tests/admin*; `bun run build` OK.
