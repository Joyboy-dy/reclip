# Phase 4 : agents IA (WebMCP et serveur MCP) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un agent IA peut utiliser ReClip de deux façons : dans le navigateur, via WebMCP sur la page publique (sans clé, comme un visiteur, les cartes apparaissent à l'écran), et à distance, via un serveur MCP `/mcp` authentifié par clé API. Les deux exposent les mêmes six outils, dont une action « transcript » qui renvoie les sous-titres en texte.

**Architecture:** La définition des outils (nom, description, schéma JSON des entrées) vit dans `crates/shared` et est exportée en JSON pour `web/`. Le cœur gagne une route `POST /api/v1/transcript` (sous-titres yt-dlp, sans téléchargement du média). `/mcp` est un serveur MCP en HTTP « streamable » (crate `rmcp`) qui appelle les mêmes services que l'API. WebMCP enregistre les outils dans la page et passe par les actions des boutons.

**Tech Stack:** Rust (`rmcp` serveur streamable HTTP, `schemars`), yt-dlp (`--write-subs --write-auto-subs --skip-download`), React + TypeScript (WebMCP `navigator.modelContext`), Playwright.

**Spec:** `docs/superpowers/specs/2026-11-11-reclip-platform-design.md` §5.2, §5.3 ; `docs/superpowers/specs/2026-11-12-phase3b-decisions.md` (#60 transcript, WebMCP sans clé).

## Global Constraints

- Six outils, mêmes noms, descriptions et schémas partout : `probe_media`, `start_download`, `list_downloads`, `get_download`, `cancel_download`, `get_transcript`.
- `/mcp` : clé API obligatoire (`Authorization: Bearer rck_…`), droits et limites de la clé appliqués comme sur l'API ; sans clé ou clé invalide → 401.
- WebMCP : aucune clé exposée ; utilise le cookie du visiteur et les limites visiteur ; rien n'est enregistré si l'API n'existe pas.
- `get_download` renvoie, quand le fichier est prêt : sur `/mcp` un lien signé `/d/{jeton}` absolu, dans WebMCP l'URL `/api/v1/jobs/{id}/file`.
- Transcript : droit `probe`, compté dans le seau « analyses » ; aucun fichier média téléchargé ; texte brut sans horodatages ni doublons des sous-titres automatiques ; 1 Mo de texte au plus.
- `RECLIP_WEBMCP_TRIAL_TOKEN` (vide par défaut) : quand il est défini, les pages HTML portent `<meta http-equiv="origin-trial" content="…">`.
- Fichiers ≤ ~300 lignes ; types d'API dans `crates/shared`, `types.gen.ts` régénéré ; CSP inchangée (aucun script inline).
- Ne jamais lire, afficher, modifier ni committer `deploy/.env`.

## Review Focus

1. **Un agent `/mcp` appelle `get_download` sur une tâche d'une autre clé** : refus (introuvable), jamais de lien. → test dans Task B.
2. **Vidéo sans sous-titres** : `get_transcript` renvoie une erreur claire (« No subtitles available »), pas un texte vide. → test dans Task A.
3. **Navigateur sans WebMCP** : la page fonctionne comme avant, aucune erreur en console. → Playwright dans Task C.
4. **Agent WebMCP qui lance 5 téléchargements** : 5 cartes visibles, gérées comme celles des boutons (annulation, reprise). → test Bun dans Task C.
5. **Sous-titres automatiques YouTube** (lignes répétées d'un bloc à l'autre) : le texte final ne contient pas de doublons. → test dans Task A.

---

## Vague 1

### Task A: Contrats des outils et route transcript

**Files:** `crates/shared/src/tools.rs` (+ export), `crates/shared/tests/export_ts.rs` (+ `web/src/api/tools.gen.json`), `crates/shared/src/api.rs` (`TranscriptRequest`, `Transcript`), `crates/core/src/ytdlp/{subs,args,runner}.rs`, `crates/core/src/http/transcript.rs`, `crates/core/src/app.rs` (route), tests `crates/core/tests/transcript.rs`, fake-ytdlp (sous-titres VTT factices).

**Produces:**
- `reclip_shared::tools::ToolDef { name, description, input_schema: serde_json::Value }` et `pub fn tool_defs() -> Vec<ToolDef>` (6 outils, schémas JSON Schema draft 2020-12 écrits à la main ou via `schemars`) ; export JSON vers `web/src/api/tools.gen.json` par le même test que `types.gen.ts` (même garde « à jour »).
- Entrées : `probe_media {url}`, `start_download {url, format?: MediaFormat, quality?: string (format_id), clip?: {start, end?}, strip_metadata?: bool, playlist_item?: int ≥ 1}`, `list_downloads {}`, `get_download {id}`, `cancel_download {id}`, `get_transcript {url, language?: string (code BCP-47, défaut : langue originale puis "en")}`.
- `TranscriptRequest { url, language: Option<String> }`, `Transcript { language: String, automatic: bool, text: String }`.
- `POST /api/v1/transcript` (droit `probe`, garde SSRF, seau analyses) : yt-dlp `--skip-download --write-subs --write-auto-subs --sub-langs <lang> --sub-format vtt -o <tmp>/sub` dans un dossier temporaire supprimé après ; préférer les sous-titres manuels aux automatiques ; conversion VTT → texte (retirer en-têtes, horodatages, balises `<…>`, lignes vides ; fusionner les lignes répétées consécutives) ; 1 Mo max ; aucun sous-titre → 422 `unavailable` « No subtitles available ».
- Tests : `vtt_to_text_strips_timing_and_tags`, `auto_captions_are_deduplicated`, `prefers_manual_over_automatic`, `no_subtitles_is_unavailable`, `transcript_counts_as_a_probe`, `transcript_needs_probe_scope`, `tools_json_is_up_to_date`.
- Commit `feat: shared tool definitions and transcript route`.

## Vague 2 (en parallèle)

### Task B: Serveur MCP `/mcp`

**Files:** `crates/core/Cargo.toml` (`rmcp` avec les fonctionnalités serveur et transport streamable HTTP), `crates/core/src/mcp/{mod,tools}.rs`, `crates/core/src/app.rs` (montage), tests `crates/core/tests/mcp.rs`.

- Monter le serveur MCP streamable HTTP sur `/mcp` (GET/POST/DELETE selon le transport). Authentification : même résolution de clé que `caller_layer` (réutiliser `access::keys::verify_key` et la construction du `Caller` pour une clé) ; visiteur sans clé → 401 JSON ; délégation Telegram refusée sur `/mcp`.
- `list_tools` renvoie exactement `tool_defs()`. Chaque outil applique les mêmes contrôles que la route HTTP équivalente (droits, SSRF, quotas avec `admission` + `check_create`, validations) en appelant les mêmes fonctions — pas de logique dupliquée : extraire au besoin des fonctions de service partagées depuis `http/{probe,jobs,transcript}.rs`.
- Résultats : JSON texte (`structuredContent` si `rmcp` le permet) ; `get_download` ajoute `download_url` (lien signé absolu, base comme `http/links.rs`) quand la tâche est `done` ; erreurs d'API → résultat d'outil en erreur avec `{code, message}`.
- Tests via un client MCP HTTP minimal (initialize, tools/list, tools/call) : `mcp_requires_a_key`, `tools_list_matches_shared_defs`, `start_and_get_download_with_signed_link`, `other_keys_jobs_are_not_found`, `mcp_respects_scopes_and_quotas`, `transcript_tool_returns_text`.
- Commit `feat(core): mcp server for agents`.

### Task C: WebMCP sur la page publique

**Files:** `web/src/public/webmcp.ts` (+ appel au démarrage), `crates/core/src/http/pages.rs` + `config.rs` (`RECLIP_WEBMCP_TRIAL_TOKEN` → meta origin-trial), `web/index.html` (emplacement `{{WEBMCP_META}}`), tests `web/tests/webmcp.test.ts`, `web/e2e/*`.

- Détection : `navigator.modelContext` (repli `document.modelContext`) ; vérifier la forme actuelle de l'API (WebMCP, Chrome) dans sa documentation publique et l'indiquer dans le rapport ; si absente, ne rien faire.
- Enregistrer les 6 outils depuis `tools.gen.json` ; chaque `execute` passe par les actions existantes (`fetchLinks`/`probeCard`, `addAndDownload`/`download`, `cancel`, store) pour que les cartes apparaissent et suivent le même cycle de vie ; `get_transcript` appelle `POST /api/v1/transcript`.
- Le serveur remplace `{{WEBMCP_META}}` par la balise origin-trial (valeur échappée) quand le jeton est défini, sinon par rien.
- Tests Bun avec un faux `navigator.modelContext` : 6 outils enregistrés avec les bons schémas ; `start_download` crée une carte visible ; `cancel_download` passe par l'action d'annulation ; sans API, aucun enregistrement. Rust : meta présente seulement si le jeton est défini, et échappée. Playwright : aucune erreur console sans WebMCP.
- Commit `feat(web): webmcp tools on the public page`.

## Vague 3

### Task D: Documentation et déploiement

**Files:** `README.md` (section « Use with AI agents » : `/mcp` avec Claude Code `claude mcp add --transport http reclip https://<domaine>/mcp --header "Authorization: Bearer rck_…"`, Claude Desktop, autres clients ; WebMCP ; liste des 6 outils), `deploy/{compose.yaml,.env.example,README.md}` (`RECLIP_WEBMCP_TRIAL_TOKEN`).
- Commit `docs: mcp server and webmcp`.
