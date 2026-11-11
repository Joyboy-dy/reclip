"""Options shared by every yt-dlp call ReClip makes.

Configured through environment variables:

- ``RECLIP_COOKIES``: path to a Netscape-format cookies.txt. Needed when YouTube
  answers "Sign in to confirm you're not a bot", which happens to most server /
  datacenter IPs. A per-call copy is used so concurrent jobs never write to the
  same file and the original stays untouched.
- ``RECLIP_PROXY``: proxy URL passed to ``--proxy`` (e.g. ``socks5://host:1080``).
- ``RECLIP_JS_RUNTIME``: force the JavaScript runtime used to solve YouTube's
  challenges (``deno``, ``bun``, ``node``, ``quickjs``, optionally ``name:/path``).
  By default it is detected: deno (yt-dlp's default), then bun, then node.
"""

import contextlib
import os
import shutil
import tempfile

BOT_CHECK_MARKER = "not a bot"


def js_runtime_args(env=None, which=shutil.which):
    env = os.environ if env is None else env
    forced = env.get("RECLIP_JS_RUNTIME", "").strip()
    if forced:
        return ["--js-runtimes", forced]
    if which("deno"):
        return []  # yt-dlp enables deno on its own
    for runtime in ("bun", "node"):
        if which(runtime):
            return ["--js-runtimes", runtime]
    return []


def _cookies_path(env):
    path = env.get("RECLIP_COOKIES", "").strip()
    return path if path and os.path.isfile(path) else None


@contextlib.contextmanager
def session_args(env=None, which=shutil.which):
    """Yield the extra yt-dlp arguments for one call, cleaning up afterwards."""
    env = os.environ if env is None else env
    args = js_runtime_args(env, which)

    proxy = env.get("RECLIP_PROXY", "").strip()
    if proxy:
        args += ["--proxy", proxy]

    cookies_copy = None
    cookies = _cookies_path(env)
    if cookies:
        fd, cookies_copy = tempfile.mkstemp(prefix="reclip-cookies-", suffix=".txt")
        os.close(fd)
        shutil.copyfile(cookies, cookies_copy)
        args += ["--cookies", cookies_copy]

    try:
        yield args
    finally:
        if cookies_copy:
            with contextlib.suppress(OSError):
                os.remove(cookies_copy)


def error_message(stderr, env=None):
    """Turn yt-dlp's stderr into the message shown to the user."""
    env = os.environ if env is None else env
    lines = [line for line in stderr.strip().splitlines() if line.strip()]
    if not lines:
        return "yt-dlp failed without an error message"
    message = lines[-1].strip()

    if BOT_CHECK_MARKER in message:
        if _cookies_path(env):
            message += (" (the cookies in RECLIP_COOKIES were rejected or have expired:"
                        " export fresh ones from a logged-in browser)")
        else:
            message += (" (YouTube is blocking this server's IP: set RECLIP_COOKIES"
                        " to a cookies.txt exported from a logged-in browser)")
    return message
