import os

import ytdlp_options as yo


def fake_which(available):
    return lambda name: f"/usr/bin/{name}" if name in available else None


# --- JS runtime -------------------------------------------------------------

def test_deno_needs_no_flag_because_ytdlp_enables_it_by_default():
    assert yo.js_runtime_args(env={}, which=fake_which({"deno", "bun"})) == []


def test_bun_is_enabled_when_deno_is_missing():
    assert yo.js_runtime_args(env={}, which=fake_which({"bun", "node"})) == ["--js-runtimes", "bun"]


def test_node_is_the_last_fallback():
    assert yo.js_runtime_args(env={}, which=fake_which({"node"})) == ["--js-runtimes", "node"]


def test_no_runtime_found_adds_nothing():
    assert yo.js_runtime_args(env={}, which=fake_which(set())) == []


def test_env_overrides_runtime_detection():
    env = {"RECLIP_JS_RUNTIME": "quickjs:/opt/qjs"}
    assert yo.js_runtime_args(env=env, which=fake_which({"bun"})) == ["--js-runtimes", "quickjs:/opt/qjs"]


# --- session args (proxy + cookies) -----------------------------------------

def test_proxy_is_passed_through():
    with yo.session_args(env={"RECLIP_PROXY": "socks5://127.0.0.1:7897"}, which=fake_which(set())) as args:
        assert args == ["--proxy", "socks5://127.0.0.1:7897"]


def test_cookies_are_copied_to_a_temp_file_that_is_removed_afterwards(tmp_path):
    src = tmp_path / "cookies.txt"
    src.write_text("# Netscape HTTP Cookie File\n")
    with yo.session_args(env={"RECLIP_COOKIES": str(src)}, which=fake_which(set())) as args:
        assert args[0] == "--cookies"
        copy = args[1]
        assert copy != str(src)
        assert open(copy).read() == "# Netscape HTTP Cookie File\n"
    assert not os.path.exists(copy)
    assert src.exists()


def test_missing_cookies_file_is_ignored(tmp_path):
    env = {"RECLIP_COOKIES": str(tmp_path / "nope.txt")}
    with yo.session_args(env=env, which=fake_which(set())) as args:
        assert args == []


# --- error messages ---------------------------------------------------------

BOT_STDERR = (
    "[youtube] abc: Downloading webpage\n"
    "ERROR: [youtube] abc: Sign in to confirm you’re not a bot. Use --cookies-from-browser or --cookies\n"
)


def test_error_message_keeps_the_last_line():
    assert yo.error_message("WARNING: x\nERROR: boom\n", env={}) == "ERROR: boom"


def test_bot_check_without_cookies_explains_how_to_fix():
    msg = yo.error_message(BOT_STDERR, env={})
    assert "not a bot" in msg
    assert "RECLIP_COOKIES" in msg


def test_bot_check_with_cookies_says_they_were_rejected(tmp_path):
    src = tmp_path / "cookies.txt"
    src.write_text("x")
    msg = yo.error_message(BOT_STDERR, env={"RECLIP_COOKIES": str(src)})
    assert "expired" in msg


def test_empty_stderr_gives_a_generic_message():
    assert yo.error_message("", env={}) == "yt-dlp failed without an error message"
