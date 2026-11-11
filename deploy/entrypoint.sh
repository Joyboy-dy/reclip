#!/bin/sh
# Keep yt-dlp fresh on container start: sites break its extractors often and
# the usual fix is updating it. The binary lives in /opt/yt-dlp, owned by the
# reclip user, so it can replace itself. Skip with RECLIP_NO_UPDATE=1.
if [ -z "$RECLIP_NO_UPDATE" ]; then
    echo "Updating yt-dlp..."
    yt-dlp -U || echo "  (couldn't update yt-dlp, continuing with the installed version)"
fi

exec "$@"
