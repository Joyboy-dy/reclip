# ReClip

A self-hosted, open-source video and audio downloader with a clean web UI. Paste links from YouTube, TikTok, Instagram, Twitter/X, and 1000+ other sites — download as MP4 or MP3.

![License](https://img.shields.io/badge/license-MIT-green)

https://github.com/user-attachments/assets/419d3e50-c933-444b-8cab-a9724986ba05

![ReClip MP3 Mode](assets/preview-mp3.png)

## Features

- Download videos from 1000+ supported sites (via [yt-dlp](https://github.com/yt-dlp/yt-dlp))
- MP4 video or MP3 audio extraction
- Quality/resolution picker
- Bulk downloads — paste multiple URLs at once
- Automatic URL deduplication
- Live progress for each download, with cancel and remove
- Cards restored after a page refresh
- Fair download queue: global and per-site concurrency limits
- Finished files kept for a set time (`RECLIP_FILE_TTL`), then deleted
- Clean, responsive UI

## Quick Start

```bash
git clone https://github.com/Joyboy-dy/reclip.git
cd reclip/deploy
cp .env.example .env    # optional: adjust the values
docker compose up -d --build
```

Open **http://localhost:8899**. The image bundles yt-dlp, Deno and ffmpeg. More details (in French) in [deploy/README.md](deploy/README.md).

## Configuration

Settings are environment variables. Copy [deploy/.env.example](deploy/.env.example) to `deploy/.env` to set them with Docker Compose.

| Variable | Purpose | Default |
|---|---|---|
| `RECLIP_PORT` | Port to listen on (`PORT` is used as a fallback). | `8899` |
| `RECLIP_HOST` | Listen address. | `0.0.0.0` |
| `RECLIP_DATA_DIR` | Data directory (SQLite database, downloads). | `./data` |
| `RECLIP_DOWNLOADS_DIR` | Downloaded files. | `<data dir>/downloads` |
| `RECLIP_WEB_DIR` | Built frontend to serve. | `./web/dist` |
| `RECLIP_YTDLP_BIN` | yt-dlp binary to run. | `yt-dlp` |
| `RECLIP_COOKIES` | Path to a Netscape `cookies.txt`, passed to yt-dlp. Required for YouTube on most servers. | none |
| `RECLIP_PROXY` | Proxy for every yt-dlp call, e.g. `socks5://127.0.0.1:7897`. | none |
| `RECLIP_JS_RUNTIME` | Force the JS runtime for YouTube challenges (`deno`, `bun`, `node`, `quickjs`, or `name:/path`). Detected automatically otherwise. | auto |
| `RECLIP_JOB_TIMEOUT` | Maximum duration of a job, in seconds. | `1800` |
| `RECLIP_PLAYLIST_LIMIT` | Maximum number of videos taken from a playlist. | `50` |
| `RECLIP_MAX_FILESIZE` | Maximum size of a downloaded file (e.g. `500M`, `2G`). | `2G` |
| `RECLIP_MAX_DURATION` | Maximum video duration, in seconds. | `10800` |
| `RECLIP_MAX_CONCURRENT` | Downloads running at once, all sites together (minimum 1). | `4` |
| `RECLIP_MAX_PER_SITE` | Downloads running at once for one site (minimum 1). | `2` |
| `RECLIP_FILE_TTL` | Time a finished file stays downloadable, in seconds. Expired files are deleted. | `21600` |
| `RECLIP_CLEANUP_INTERVAL` | Period of the expiry sweep, in seconds. | `600` |
| `RECLIP_TELEGRAM_BOT_USERNAME` | Telegram bot name, without the `@` (later phase). | disabled |
| `RECLIP_PUBLIC_URL` | Public address behind a reverse proxy, e.g. `https://reclip.example.com`: scheme and host only, no path (otherwise it is ignored with a warning). Used for signed links and the `Origin` check. If unset, it is learned from the first proxied request (signed links only). | none |
| `RECLIP_TRUSTED_PROXIES` | Comma-separated IPs or CIDR ranges whose `X-Forwarded-*` headers are trusted. | loopback and private ranges |
| `RECLIP_ADMIN_TOKEN` | Token for the admin API (32 random characters or more). If unset, one is generated at first start and printed once in the logs. | generated |
| `RECLIP_ADMIN_EMAILS` | Comma-separated lowercase emails allowed to sign in to the admin page (`/admin`). | none |
| `RECLIP_ADMIN_DEFAULT_PASSWORD` | Initial password of each admin, used only for their first sign-in; they must then choose a new one of 12 characters or more. Remove it after the first sign-in and never commit it. | none |
| `RECLIP_PUBLIC_WEB` | `false` turns on private mode: the whole API needs a key, except `GET /api/v1/config`. | `true` |
| `RECLIP_LINK_TTL` | Lifetime of a signed download link, in seconds. | `21600` |
| `RECLIP_MAX_DISK` | Maximum size of the downloads directory (e.g. `5G`, `500M`, or bytes). When full, the oldest finished files are deleted first. The cap is soft: downloads in progress and the final ffmpeg copy can exceed it briefly. | `5G` |
| `RECLIP_HISTORY_DAYS` | Days a job title stays in the history, even after its file is deleted. | `30` |
| `RECLIP_CONCURRENT_FRAGMENTS` | Fragments downloaded in parallel for segmented streams (HLS, DASH). | `4` |
| `RECLIP_IMPERSONATE` | Browser fingerprint sent to yt-dlp, needed for some sites such as Instagram and TikTok. Empty disables it. | `chrome` |
| `RECLIP_LIMIT_<TIER>_<LIMIT>` | Rate limits. `TIER` is `WEB`, `KEY` or `TG`; `LIMIT` is `PROBES_PER_MIN`, `JOBS_PER_HOUR` or `ACTIVE`. `0` means unlimited. | see below |
| `RECLIP_NO_UPDATE` | Container only: skip the yt-dlp update on start. | unset |

Default limits (probes per minute / jobs per hour / active jobs): `WEB` 120 / 300 / 200, `KEY` 120 / 200 / 100, `TG` 30 / 40 / 10. Browser visitors are counted per IP address (per /64 prefix for IPv6). When a download is refused with a `Retry-After` of 120 seconds or less, the web UI waits and retries, up to 3 times.

The downloads directory belongs to ReClip: at startup it deletes the job folders (32 hex characters) it no longer knows, so give it a dedicated folder.

### Admin page

Admins sign in at `/admin`. The page is not linked from the site, so share the address only with admins. Set `RECLIP_ADMIN_EMAILS` and `RECLIP_ADMIN_DEFAULT_PASSWORD`: each admin signs in once with the default password and is then asked to choose their own. The admin token and the `reclip-core keys` CLI keep working for scripts. Details (in French) in [deploy/README.md](deploy/README.md).

### YouTube on a server

YouTube refuses most datacenter IPs with *"Sign in to confirm you're not a bot"*. To get past it:

1. Use a secondary Google account: accounts used with yt-dlp can get flagged.
2. In a private browser window, log in to YouTube, export the cookies with an extension such as *Get cookies.txt LOCALLY*, then close the window without browsing further (YouTube rotates the cookies of open sessions).
3. Copy the file to `deploy/cookies.txt`, uncomment the `cookies.txt` volume line in `deploy/compose.yaml`, and set `RECLIP_COOKIES=/run/secrets/cookies.txt` in `deploy/.env`.

ReClip uses a per-request copy, so the original file is never modified. When the cookies expire, the error message says so: export new ones.

YouTube also requires a JavaScript runtime: the Docker image ships Deno. When running outside Docker, install [Deno](https://deno.com) (recommended by yt-dlp). Bun 1.2.11 to 1.3.14 and Node also work as fallbacks.

## Usage

1. Paste one or more video URLs into the input box
2. Choose **MP4** (video) or **MP3** (audio)
3. Click **Fetch** to load video info and thumbnails
4. Select quality/resolution if available
5. Click **Download** on individual videos, or **Download All**

Progress updates live. You can cancel a running download or remove a card, and cards come back after a page refresh.

### Using the API

Scripts and agents use an API key: see [API for agents](#api-for-agents). Without a key, the API treats the caller as a browser: POST and DELETE requests need an `Origin` header naming this site, so a plain `curl` gets `403 forbidden`.

## API for agents

Scripts and bots use API keys instead of the browser cookie. Create one inside the container (the key is shown once):

```bash
docker compose exec core reclip-core keys create --name agent
```

Options: `--scopes probe,jobs:create,jobs:read,jobs:cancel` (the default set; `delegate` lets a bot act for Telegram users and must be added explicitly), `--unlimited`, `--expires-days N` (1 to 36500). Also `keys list` and `keys revoke <id>`. Keys look like `rck_<id>_<secret>` and go in the `Authorization: Bearer` header. Other schemes (such as Basic credentials forwarded by a proxy) are ignored: the request is handled as a browser visitor. The examples use a placeholder host and key:

```bash
export RECLIP=https://reclip.example.com
export KEY=rck_xxxxxxxx_...

# Probe a URL
curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"url": "https://www.youtube.com/watch?v=..."}' $RECLIP/api/v1/probe

# Create a job (format: "mp4" or "mp3"); answers 202 with the job id
curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"url": "https://www.youtube.com/watch?v=...", "format": "mp4"}' $RECLIP/api/v1/jobs

# Follow it: poll, or stream server-sent events
curl -H "Authorization: Bearer $KEY" $RECLIP/api/v1/jobs/<id>
curl -N -H "Authorization: Bearer $KEY" $RECLIP/api/v1/jobs/<id>/events

# Download the file
curl -H "Authorization: Bearer $KEY" -OJ $RECLIP/api/v1/jobs/<id>/file

# Get a signed link that works without a key: {"url": ..., "expires_at": ...}
curl -X POST -H "Authorization: Bearer $KEY" $RECLIP/api/v1/jobs/<id>/link

# Cancel, or delete
curl -X POST -H "Authorization: Bearer $KEY" $RECLIP/api/v1/jobs/<id>/cancel
curl -X DELETE -H "Authorization: Bearer $KEY" $RECLIP/api/v1/jobs/<id>
```

A signed link (`GET /d/{token}`) needs no key and expires after `RECLIP_LINK_TTL` seconds, or when the file is deleted.

A key with the `delegate` scope can act for a Telegram user with the `X-Reclip-On-Behalf-Of: tg:<id>` header. Every delegate key can see all `tg:*` jobs, so only give it to your own bot.

Errors:

| Status | Code | Meaning |
|---|---|---|
| 401 | `unauthorized` | Invalid, expired or revoked key; no key in private mode; wrong admin token on the admin API. |
| 403 | `forbidden` | The key lacks the scope, the key cannot act for users (no `delegate`), or a request without a key came with a cross-site or missing `Origin`. |
| 403 | `forbidden_target` | The URL points to a private or internal address. |
| 429 | `rate_limited` | Over a limit. `Retry-After` gives the wait in seconds. |

Responses also carry `RateLimit-Limit`, `RateLimit-Remaining` and `RateLimit-Reset` (seconds until the window resets).

With `RECLIP_PUBLIC_WEB=false`, the whole API needs a key except `GET /api/v1/config`. Behind a reverse proxy, set `RECLIP_PUBLIC_URL` and make sure the proxy sends `X-Forwarded-Proto`, otherwise POST and DELETE requests from browsers are refused. With nginx, the default `proxy_set_header Host $proxy_host` without `X-Forwarded-Host` also triggers this 403 unless `RECLIP_PUBLIC_URL` is set. See [deploy/README.md](deploy/README.md) (in French) for the admin token, limits and proxy setup.

## Development

You need Rust (see `rust-toolchain.toml`), [Bun](https://bun.sh), and yt-dlp, Deno and ffmpeg on your machine.

```bash
# Backend (serves web/dist on http://localhost:8899)
RECLIP_YTDLP_BIN=/path/to/yt-dlp cargo run -p reclip-core

# Frontend
cd web && bun install && bun run dev

# Tests
cargo test --workspace
cd web && bun test
```

## Supported Sites

Anything [yt-dlp supports](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md), including:

YouTube, TikTok, Instagram, Twitter/X, Reddit, Facebook, Vimeo, Twitch, Dailymotion, SoundCloud, Loom, Streamable, Pinterest, Tumblr, Threads, LinkedIn, and many more.

## Stack

- **Backend:** Rust ([axum](https://github.com/tokio-rs/axum)), state in SQLite
- **Frontend:** TypeScript, built with Bun
- **Download engine:** [yt-dlp](https://github.com/yt-dlp/yt-dlp) standalone binary, [Deno](https://deno.com) (YouTube challenges) and [ffmpeg](https://ffmpeg.org/)

## Disclaimer

This tool is intended for personal use only. Please respect copyright laws and the terms of service of the platforms you download from. The developers are not responsible for any misuse of this tool.

## License

[MIT](LICENSE)
