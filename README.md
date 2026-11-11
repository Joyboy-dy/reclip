# ReClip

A self-hosted, open-source video and audio downloader with a clean web UI. Paste links from YouTube, TikTok, Instagram, Twitter/X, and 1000+ other sites — download as MP4 or MP3.

![Python](https://img.shields.io/badge/python-3.8+-blue)
![License](https://img.shields.io/badge/license-MIT-green)

https://github.com/user-attachments/assets/419d3e50-c933-444b-8cab-a9724986ba05

![ReClip MP3 Mode](assets/preview-mp3.png)

## Features

- Download videos from 1000+ supported sites (via [yt-dlp](https://github.com/yt-dlp/yt-dlp))
- MP4 video or MP3 audio extraction
- Quality/resolution picker
- Bulk downloads — paste multiple URLs at once
- Automatic URL deduplication
- Clean, responsive UI — no frameworks, no build step
- Single Python file backend (~150 lines)

## Quick Start

```bash
brew install yt-dlp ffmpeg    # or apt install ffmpeg && pip install yt-dlp
git clone https://github.com/averygan/reclip.git
cd reclip
./reclip.sh
```

Open **http://localhost:8899**.

Or with Docker:

```bash
docker build -t reclip . && docker run -p 8899:8899 reclip
```

## Configuration

| Variable | Purpose |
|---|---|
| `RECLIP_COOKIES` | Path to a Netscape `cookies.txt`, passed to yt-dlp. Required for YouTube on most servers. |
| `RECLIP_PROXY` | Proxy for every yt-dlp call, e.g. `socks5://127.0.0.1:7897`. |
| `RECLIP_JS_RUNTIME` | Force the JS runtime for YouTube challenges (`deno`, `bun`, `node`, `quickjs`, or `name:/path`). Detected automatically otherwise. |
| `RECLIP_NO_UPDATE` | Skip the yt-dlp update on start. |

### YouTube on a server

YouTube refuses most datacenter IPs with *"Sign in to confirm you're not a bot"*. To get past it:

1. Use a secondary Google account: accounts used with yt-dlp can get flagged.
2. In a private browser window, log in to YouTube, export the cookies with an extension such as *Get cookies.txt LOCALLY*, then close the window without browsing further (YouTube rotates the cookies of open sessions).
3. Copy the file to the server and point `RECLIP_COOKIES` to it (with Docker Compose, uncomment the lines in `docker-compose.yml`).

ReClip uses a per-request copy, so the original file is never modified. When the cookies expire, the error message says so: export new ones.

YouTube also requires a JavaScript runtime: the Docker image ships Deno. When running `reclip.sh`, install [Deno](https://deno.com) (recommended by yt-dlp). Bun 1.2.11 to 1.3.14 and Node also work as fallbacks.

## Usage

1. Paste one or more video URLs into the input box
2. Choose **MP4** (video) or **MP3** (audio)
3. Click **Fetch** to load video info and thumbnails
4. Select quality/resolution if available
5. Click **Download** on individual videos, or **Download All**

## Supported Sites

Anything [yt-dlp supports](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md), including:

YouTube, TikTok, Instagram, Twitter/X, Reddit, Facebook, Vimeo, Twitch, Dailymotion, SoundCloud, Loom, Streamable, Pinterest, Tumblr, Threads, LinkedIn, and many more.

## Stack

- **Backend:** Python + Flask (~150 lines)
- **Frontend:** Vanilla HTML/CSS/JS (single file, no build step)
- **Download engine:** [yt-dlp](https://github.com/yt-dlp/yt-dlp) + [ffmpeg](https://ffmpeg.org/)
- **Dependencies:** 2 (Flask, yt-dlp with yt-dlp-ejs), plus a JS runtime (Deno in Docker) for YouTube

## Disclaimer

This tool is intended for personal use only. Please respect copyright laws and the terms of service of the platforms you download from. The developers are not responsible for any misuse of this tool.

## License

[MIT](LICENSE)
