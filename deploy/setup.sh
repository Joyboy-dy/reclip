#!/bin/sh
# Creates a .env file for the ReClip Telegram bot.
#
#   deploy/setup.sh [--no-logout] [path/to/.env]            (default path: ./.env)
#   deploy/setup.sh --update [--no-logout] path/to/.env     (existing file)
#
# Reads TELEGRAM_BOT_TOKEN, TELEGRAM_API_ID, TELEGRAM_API_HASH and
# RECLIP_PUBLIC_URL from the environment, and asks for the ones that are missing.
# Without an api_id it sets up the simple mode (50 MB uploads). With one, it sets
# up the local Bot API server (2000 MB) and, unless --no-logout is given, calls
# logOut once on https://api.telegram.org, as Telegram requires before switching.
# The bot token is never printed.
#
# --update switches an existing .env to the local server mode: the file is copied
# to <file>.bak-<unix time>, then only TELEGRAM_API_ID, TELEGRAM_API_HASH,
# TELEGRAM_API_URL and COMPOSE_PROFILES are rewritten. Every other line is kept.
# The token comes from the environment or from that file.

set -eu

usage() {
    echo "Usage: $0 [--update] [--no-logout] [path/to/.env]" >&2
    exit 2
}

logout=1
update=0
target=""
for arg in "$@"; do
    case "$arg" in
        --no-logout) logout=0 ;;
        --update) update=1 ;;
        -h | --help) usage ;;
        -*) usage ;;
        *)
            [ -z "$target" ] || usage
            target=$arg
            ;;
    esac
done
[ -n "$target" ] || target=./.env

created=0
backup=""
tmp=""
cleanup() {
    stty echo 2>/dev/null || true
    # Remove the placeholder file if we stopped before writing the real one.
    if [ "$created" = 1 ]; then
        rm -f "$target"
    fi
    # An update that stopped early changed nothing: drop its backup and temp file.
    [ -z "$tmp" ] || rm -f "$tmp"
    [ -z "$backup" ] || rm -f "$backup"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [ "$update" = 1 ]; then
    if [ ! -f "$target" ] || [ -L "$target" ]; then
        echo "--update needs an existing regular file: $target. Nothing was written." >&2
        exit 1
    fi
elif [ -e "$target" ] || [ -L "$target" ]; then
    echo "$target already exists. Use --update to switch it to the local server, or pick another path: nothing was written." >&2
    exit 1
fi

# ask VAR "prompt" [secret]: keeps the value of VAR if already set, else asks.
# shellcheck disable=SC2034 # "value" is copied into the named variable by eval
ask() {
    name=$1
    prompt=$2
    secret=${3:-}
    eval "current=\${$name:-}"
    [ -z "$current" ] || return 0
    if [ ! -t 0 ]; then
        return 0
    fi
    printf '%s' "$prompt" >&2
    if [ -n "$secret" ]; then
        stty -echo 2>/dev/null || true
        IFS= read -r value || value=""
        stty echo 2>/dev/null || true
        echo >&2
    else
        IFS= read -r value || value=""
    fi
    eval "$name=\$value"
}

TELEGRAM_BOT_TOKEN=${TELEGRAM_BOT_TOKEN:-}
TELEGRAM_API_ID=${TELEGRAM_API_ID:-}
TELEGRAM_API_HASH=${TELEGRAM_API_HASH:-}
RECLIP_PUBLIC_URL=${RECLIP_PUBLIC_URL:-}

if [ "$update" = 1 ]; then
    # The token may already be in the file; the last assignment wins.
    if [ -z "$TELEGRAM_BOT_TOKEN" ]; then
        TELEGRAM_BOT_TOKEN=$(sed -n 's/^TELEGRAM_BOT_TOKEN=//p' "$target" | tail -n 1)
    fi
    ask TELEGRAM_API_ID "api_id from my.telegram.org: "
    if [ -z "$TELEGRAM_API_ID" ]; then
        echo "--update needs TELEGRAM_API_ID (and TELEGRAM_API_HASH)." >&2
        exit 1
    fi
    ask TELEGRAM_API_HASH "api_hash from my.telegram.org: " secret
    # RECLIP_PUBLIC_URL stays as it is in the file.
    RECLIP_PUBLIC_URL=""
else
    ask TELEGRAM_BOT_TOKEN "Bot token from @BotFather: " secret
    ask TELEGRAM_API_ID "api_id from my.telegram.org (empty = simple mode, 50 MB): "
    if [ -n "$TELEGRAM_API_ID" ]; then
        ask TELEGRAM_API_HASH "api_hash from my.telegram.org: " secret
    fi
    ask RECLIP_PUBLIC_URL "Public address of ReClip, e.g. https://reclip.example.com (optional): "
fi

case "$TELEGRAM_BOT_TOKEN" in
    "")
        echo "TELEGRAM_BOT_TOKEN is required." >&2
        exit 1
        ;;
    *[!A-Za-z0-9:_-]* | *:*:* | :* | *:)
        echo "TELEGRAM_BOT_TOKEN does not look like a bot token." >&2
        exit 1
        ;;
    *:*) ;;
    *)
        echo "TELEGRAM_BOT_TOKEN does not look like a bot token." >&2
        exit 1
        ;;
esac

case "$TELEGRAM_API_ID" in
    "" | *[!0-9]*)
        if [ -n "$TELEGRAM_API_ID" ]; then
            echo "TELEGRAM_API_ID must be a number." >&2
            exit 1
        fi
        ;;
esac
if [ -n "$TELEGRAM_API_ID" ]; then
    case "$TELEGRAM_API_HASH" in
        "" | *[!0-9a-fA-F]*)
            echo "TELEGRAM_API_HASH is required with an api_id (hexadecimal)." >&2
            exit 1
            ;;
    esac
fi

case "$RECLIP_PUBLIC_URL" in
    *[!A-Za-z0-9:/._-]*)
        echo "RECLIP_PUBLIC_URL has unexpected characters." >&2
        exit 1
        ;;
esac

# Prove that the file can be created before anything irreversible happens.
# Exclusive create (noclobber): fails on an existing file or a dangling symlink.
umask 077
if [ "$update" = 1 ]; then
    backup=$target.bak-$(date +%s)
    if ! (set -C && cp "$target" "$backup") 2>/dev/null; then
        backup=""
        echo "Cannot back up $target. Nothing was changed." >&2
        exit 1
    fi
    echo "Backed up $target to $backup" >&2
elif ! (set -C && : >"$target") 2>/dev/null; then
    echo "Cannot create $target (missing directory, no permission, or it already exists). Nothing was changed." >&2
    exit 1
else
    created=1
fi

if [ -n "$TELEGRAM_API_ID" ] && [ "$logout" = 1 ]; then
    if ! command -v curl >/dev/null 2>&1; then
        echo "curl is required for logOut. Install it, or run again with --no-logout." >&2
        exit 1
    fi
    echo "Calling logOut on api.telegram.org (needed once before using the local server)..." >&2
    # The token is passed on stdin so it is not visible in the process list.
    # curl -s hides the URL from error output.
    if printf 'url = "https://api.telegram.org/bot%s/logOut"\n' "$TELEGRAM_BOT_TOKEN" \
        | curl -s -f -X POST --max-time 30 -K - -o /dev/null; then
        echo "logOut done." >&2
    else
        echo "logOut failed (bad token, network, or already logged out)." >&2
        echo "If the bot was already logged out in the last 10 minutes, run again with --no-logout." >&2
        exit 1
    fi
fi

if [ "$update" = 1 ]; then
    # Drop the old assignments of the four variables, keep every other line,
    # append the new values, then replace the file in one step.
    tmp=$(mktemp "$target.tmp.XXXXXX")
    {
        grep -Ev '^[[:space:]]*(export[[:space:]]+)?(TELEGRAM_API_ID|TELEGRAM_API_HASH|TELEGRAM_API_URL|COMPOSE_PROFILES)=' "$target" || true
        echo "TELEGRAM_API_ID=$TELEGRAM_API_ID"
        echo "TELEGRAM_API_HASH=$TELEGRAM_API_HASH"
        echo "TELEGRAM_API_URL=http://telegram-bot-api:8081"
        echo "COMPOSE_PROFILES=telegram,telegram-local"
    } >"$tmp"
    mv -f "$tmp" "$target"
    tmp=""
    backup=""
    echo "Updated $target (local server mode, uploads up to 2000 MB). Backup kept." >&2
    compose_file=$(dirname "$0")/compose.yaml
    echo "Next: docker compose -f $compose_file --env-file $target up -d" >&2
    exit 0
fi

{
    echo "# Generated by deploy/setup.sh. Contains secrets: do not commit."
    echo "TELEGRAM_BOT_TOKEN=$TELEGRAM_BOT_TOKEN"
    if [ -n "$TELEGRAM_API_ID" ]; then
        echo "TELEGRAM_API_ID=$TELEGRAM_API_ID"
        echo "TELEGRAM_API_HASH=$TELEGRAM_API_HASH"
        echo "TELEGRAM_API_URL=http://telegram-bot-api:8081"
        echo "COMPOSE_PROFILES=telegram,telegram-local"
    else
        echo "COMPOSE_PROFILES=telegram"
    fi
    if [ -n "$RECLIP_PUBLIC_URL" ]; then
        echo "RECLIP_PUBLIC_URL=$RECLIP_PUBLIC_URL"
    fi
} >"$target"
created=0

if [ -n "$TELEGRAM_API_ID" ]; then
    echo "Wrote $target (local server mode, uploads up to 2000 MB)." >&2
else
    echo "Wrote $target (simple mode, uploads up to 50 MB)." >&2
fi
compose_file=$(dirname "$0")/compose.yaml
echo "Next: docker compose -f $compose_file --env-file $target up -d" >&2
