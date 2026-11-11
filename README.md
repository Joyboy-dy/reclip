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
| `RECLIP_FILE_TTL` | Time a finished file stays downloadable, in seconds. Expired files are deleted. | `86400` |
| `RECLIP_CLEANUP_INTERVAL` | Period of the expiry sweep, in seconds. | `600` |
| `RECLIP_TELEGRAM_BOT_USERNAME` | Telegram bot name, without the `@` (later phase). | disabled |
| `RECLIP_NO_UPDATE` | Container only: skip the yt-dlp update on start. | unset |

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

Jobs are attached to an anonymous `reclip_client` cookie set by the server. A script must keep cookies between requests, or it will not see its own jobs:

```bash
curl -c cookies.txt -b cookies.txt http://localhost:8899/api/v1/jobs
```

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
