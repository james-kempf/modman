#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Read-only Nexus Mods lookups for the game-support research agent.

Usage (always run as `python3 -I`):

  nexus_mod_info.py mod <domain> <mod_id>           v1 mod page data
  nexus_mod_info.py files <domain> <mod_id>         v1 file list
  nexus_mod_info.py requirements <domain> <mod_id>  v2 GraphQL: Nexus and
                                                    off-site requirements
  nexus_mod_info.py top <domain> [n] [--trending]   most endorsed (v2), or
                                                    v1 trending
  nexus_mod_info.py contents <domain> <mod_id> [file_id]
                                                    v2 GraphQL: archive file
                                                    listing (no download)
  nexus_mod_info.py key-status                      which key source is used

The API v1 has no requirements or "most endorsed" endpoint, so those use the
v2 GraphQL endpoint. Output is compact JSON on stdout; rate-limit headers and
warnings go to stderr.

The API key is read from Vessel's own storage (Secret Service item
service=vessel username=nexus-api-key, then
$XDG_CONFIG_HOME/vessel/credentials.toml), else from
$XDG_CONFIG_HOME/vessel-game-support/nexus_api_key. It is only ever sent to
api.nexusmods.com as a header that is not forwarded on redirects, and it is
never printed.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request

API_HOST = "api.nexusmods.com"
API = f"https://{API_HOST}"
GRAPHQL = f"{API}/v2/graphql"
APP_NAME = "vessel-game-support"
APP_VERSION = "1.0"
KEYRING_ATTRS = ("service", "vessel", "username", "nexus-api-key")
MAX_TRIES = 4
TIMEOUT = 30
DOMAIN_RE = re.compile(r"^[a-z0-9]+$")
# Nexus keys are base64-like; anything else (newlines, spaces) could inject
# headers, so such a value is ignored.
KEY_RE = re.compile(r"^[A-Za-z0-9+/=_.-]{8,512}$")
RATE_HEADERS = (
    "X-RL-Hourly-Remaining",
    "X-RL-Hourly-Limit",
    "X-RL-Daily-Remaining",
    "X-RL-Daily-Limit",
    "X-RL-Hourly-Reset",
)

# Every string written to stdout/stderr passes through _redact.
_SECRETS: list[str] = []


class HelperError(Exception):
    """A clean, user-facing failure (no traceback)."""

    def __init__(self, message: str, code: int = 1):
        super().__init__(message)
        self.code = code


def _redact(text: str) -> str:
    for s in _SECRETS:
        text = text.replace(s, "<redacted>")
    return text


def warn(msg: str) -> None:
    print(_redact(f"[game-support] {msg}"), file=sys.stderr)


def emit(obj) -> None:
    print(_redact(json.dumps(obj, separators=(",", ":"), ensure_ascii=False)))


# ---- API key ---------------------------------------------------------------


def _config_home(env) -> str:
    return env.get("XDG_CONFIG_HOME") or os.path.join(
        env.get("HOME") or os.path.expanduser("~"), ".config"
    )


def key_file_path(env=None) -> str:
    return os.path.join(
        _config_home(os.environ if env is None else env),
        "vessel-game-support",
        "nexus_api_key",
    )


def vessel_credentials_path(env=None) -> str:
    return os.path.join(
        _config_home(os.environ if env is None else env), "vessel", "credentials.toml"
    )


def _read_private(path: str, *, strict: bool, warn_fn) -> str | None:
    """Reads a file only if it is a regular file we own. Group/other access
    is refused when `strict` (as Vessel does), otherwise only warned about."""
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    except FileNotFoundError:
        return None
    except OSError as e:
        warn_fn(f"cannot open {path}: {e.strerror}; ignoring it")
        return None
    with os.fdopen(fd, "r", encoding="utf-8", errors="replace") as f:
        st = os.fstat(f.fileno())
        if not stat.S_ISREG(st.st_mode) or st.st_uid != os.geteuid():
            warn_fn(f"{path} is not a regular file owned by you; ignoring it")
            return None
        mode = stat.S_IMODE(st.st_mode)
        if mode & 0o077:
            if strict:
                warn_fn(f"{path} has mode {mode:04o} (readable by others); ignoring it")
                return None
            warn_fn(f"{path} has mode {mode:04o}; it should be 0600 (chmod 600 '{path}')")
        return f.read()


def _keyring_lookup() -> str | None:
    exe = shutil.which("secret-tool")
    if not exe:
        return None
    try:
        r = subprocess.run(
            [exe, "lookup", *KEYRING_ATTRS],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return r.stdout.strip() if r.returncode == 0 else None


def _valid(key: str | None, source: str, warn_fn) -> str | None:
    if not key:
        return None
    key = key.strip()
    if KEY_RE.match(key):
        return key
    warn_fn(f"the key from {source} is malformed; ignoring it")
    return None


def load_key(env=None, keyring=_keyring_lookup, warn_fn=warn) -> tuple[str | None, str]:
    """Returns (key, source description). Never prints the key."""
    env = os.environ if env is None else env
    key = _valid(keyring() if keyring else None, "the Vessel keyring", warn_fn)
    if key:
        return _remember(key), "vessel keyring"
    creds = vessel_credentials_path(env)
    text = _read_private(creds, strict=True, warn_fn=warn_fn)
    if text is not None:
        try:
            raw = tomllib.loads(text).get("nexus_api_key")
        except tomllib.TOMLDecodeError:
            # The parse error could quote the file (the key); do not echo it.
            warn_fn(f"{creds} is not valid TOML; ignoring it")
            raw = None
        key = _valid(raw if isinstance(raw, str) else None, creds, warn_fn)
        if key:
            return _remember(key), f"vessel credentials file ({creds})"
    path = key_file_path(env)
    key = _valid(_read_private(path, strict=False, warn_fn=warn_fn), path, warn_fn)
    if key:
        return _remember(key), f"key file ({path})"
    return None, "none"


def _remember(key: str) -> str:
    if key not in _SECRETS:
        _SECRETS.append(key)
    return key


# ---- HTTP ------------------------------------------------------------------


def check_url(url: str, allowed_hosts, allow_http: bool = False) -> None:
    """HTTPS to an allowlisted host only. `allow_http` is for tests and is
    deliberately not reachable from any command-line flag."""
    p = urllib.parse.urlsplit(url)
    if p.scheme != "https" and not (allow_http and p.scheme == "http"):
        raise HelperError(f"refusing non-HTTPS URL ({p.scheme or 'no'} scheme)")
    host = (p.hostname or "").lower()
    if host not in allowed_hosts:
        raise HelperError(f"refusing host {host!r}: not in the allowlist")


class _SafeRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self, allowed_hosts, allow_http: bool):
        super().__init__()
        self.allowed_hosts = allowed_hosts
        self.allow_http = allow_http

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        check_url(newurl, self.allowed_hosts, self.allow_http)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def build_opener(allowed_hosts, allow_http: bool = False):
    return urllib.request.build_opener(_SafeRedirect(allowed_hosts, allow_http))


def report_rate(headers) -> None:
    if headers is None:
        return
    parts = [f"{h[5:].lower()}={headers.get(h)}" for h in RATE_HEADERS if headers.get(h)]
    if parts:
        warn("rate limit: " + " ".join(parts))


def _retry_delay(headers, attempt: int) -> float:
    try:
        return min(float(headers.get("Retry-After")), 120.0)
    except (TypeError, ValueError):
        return float(2 ** (attempt + 1))


def request(
    url: str,
    *,
    allowed_hosts,
    opener=None,
    key: str | None = None,
    data: bytes | None = None,
    headers: dict | None = None,
    allow_http: bool = False,
    sleep=time.sleep,
    timeout: float = TIMEOUT,
):
    """Opens `url` (retrying 429) and returns the response. The key goes only
    to the Nexus API host, as an unredirected header."""
    check_url(url, allowed_hosts, allow_http)
    if key and urllib.parse.urlsplit(url).hostname != API_HOST:
        raise HelperError("refusing to send the API key to a non-API host")
    opener = opener or build_opener(allowed_hosts, allow_http)
    hdrs = {
        "User-Agent": f"{APP_NAME}/{APP_VERSION}",
        "Application-Name": APP_NAME,
        "Application-Version": APP_VERSION,
        "Accept": "application/json",
    }
    hdrs.update(headers or {})
    for attempt in range(MAX_TRIES):
        req = urllib.request.Request(url, data=data, headers=hdrs)
        if key:
            req.add_unredirected_header("apikey", key)
        try:
            resp = opener.open(req, timeout=timeout)
        except urllib.error.HTTPError as e:
            report_rate(e.headers)
            if e.code == 429 and attempt < MAX_TRIES - 1:
                delay = _retry_delay(e.headers, attempt)
                warn(f"HTTP 429 (rate limited); retrying in {delay:g}s")
                e.close()
                sleep(delay)
                continue
            raise
        report_rate(resp.headers)
        return resp
    raise AssertionError("unreachable")


def _http_fail(e: urllib.error.HTTPError, what: str) -> HelperError:
    try:
        body = e.read(400).decode("utf-8", "replace")
    except Exception:  # noqa: BLE001 - best effort detail only
        body = ""
    detail = f": {body.strip()}" if body.strip() else ""
    if e.code == 401:
        return HelperError(f"{what}: HTTP 401, the Nexus API key was rejected{detail}")
    return HelperError(f"{what}: HTTP {e.code}{detail}")


def _json(resp) -> object:
    with resp:
        return json.loads(resp.read().decode("utf-8"))


def v1_get(path: str, key: str | None, opener=None, sleep=time.sleep):
    if not key:
        raise HelperError(
            "no Nexus API key found; v1 lookups (mod, files, top --trending) need one. "
            "requirements, top and contents work without a key."
        )
    url = f"{API}/v1/{path}"
    try:
        return _json(request(url, allowed_hosts={API_HOST}, opener=opener, key=key, sleep=sleep))
    except urllib.error.HTTPError as e:
        raise _http_fail(e, f"GET /v1/{path}") from None


def graphql(query: str, variables: dict, key: str | None, opener=None, sleep=time.sleep):
    body = json.dumps({"query": query, "variables": variables}).encode()
    try:
        out = _json(
            request(
                GRAPHQL,
                allowed_hosts={API_HOST},
                opener=opener,
                key=key,
                data=body,
                headers={"Content-Type": "application/json"},
                sleep=sleep,
            )
        )
    except urllib.error.HTTPError as e:
        raise _http_fail(e, "POST /v2/graphql") from None
    if out.get("errors"):
        msgs = "; ".join(str(err.get("message")) for err in out["errors"])
        raise HelperError(f"GraphQL error: {msgs}")
    return out.get("data") or {}


# ---- commands --------------------------------------------------------------


def _trim(text, n: int):
    if not isinstance(text, str):
        return text
    return text if len(text) <= n else text[:n] + "...[truncated]"


def _pick(d: dict, keys) -> dict:
    return {k: d.get(k) for k in keys if d.get(k) is not None}


MOD_KEYS = (
    "mod_id", "domain_name", "game_id", "name", "summary", "version", "author",
    "uploaded_by", "category_id", "endorsement_count", "mod_downloads",
    "mod_unique_downloads", "created_time", "updated_time", "status",
    "available", "contains_adult_content",
)
FILE_KEYS = (
    "file_id", "name", "version", "category_name", "is_primary", "file_name",
    "size_in_bytes", "uploaded_time", "mod_version", "external_virus_scan_url",
)


def _game_id(domain: str, key, opener, sleep) -> int:
    data = graphql(
        "query($d:String){game(domainName:$d){id}}", {"d": domain}, key, opener, sleep
    )
    game = data.get("game")
    if not game:
        raise HelperError(f"unknown Nexus domain {domain!r}")
    return int(game["id"])


def cmd_mod(a, key, opener, sleep):
    d = v1_get(f"games/{a.domain}/mods/{a.mod_id}.json", key, opener, sleep)
    out = _pick(d, MOD_KEYS)
    out["url"] = f"https://www.nexusmods.com/{a.domain}/mods/{a.mod_id}"
    out["description"] = _trim(d.get("description"), 4000)
    emit(out)


def cmd_files(a, key, opener, sleep):
    d = v1_get(f"games/{a.domain}/mods/{a.mod_id}/files.json", key, opener, sleep)
    files = []
    for f in d.get("files") or []:
        item = _pick(f, FILE_KEYS)
        item["description"] = _trim(f.get("description"), 400)
        files.append(item)
    files.sort(key=lambda f: f.get("uploaded_time") or "", reverse=True)
    emit({"domain": a.domain, "mod_id": a.mod_id, "files": files})


REQ_FIELDS = "modId modName gameId url notes externalRequirement"


def _split_requirements(nodes, game_id: int) -> dict:
    nexus, offsite = [], []
    for n in nodes or []:
        if n.get("externalRequirement"):
            offsite.append(_pick(n, ("modName", "url", "notes")))
        else:
            item = _pick(n, ("modId", "modName", "url", "notes"))
            if str(n.get("gameId")) != str(game_id):
                item["other_game_id"] = n.get("gameId")
            nexus.append(item)
    return {"nexus_requirements": nexus, "offsite_requirements": offsite}


def cmd_requirements(a, key, opener, sleep):
    gid = _game_id(a.domain, key, opener, sleep)
    data = graphql(
        "query($m:ID!,$g:ID!){mod(modId:$m,gameId:$g){modId name endorsements"
        f" modRequirements{{nexusRequirements(count:100){{totalCount nodes{{{REQ_FIELDS}}}}}"
        " modsRequiringThisMod{totalCount}}}}",
        {"m": str(a.mod_id), "g": str(gid)},
        key,
        opener,
        sleep,
    )
    mod = data.get("mod")
    if not mod:
        raise HelperError(f"mod {a.mod_id} not found on {a.domain}")
    reqs = mod.get("modRequirements") or {}
    out = {"domain": a.domain, "game_id": gid, "mod_id": a.mod_id, "name": mod.get("name")}
    out.update(_split_requirements((reqs.get("nexusRequirements") or {}).get("nodes"), gid))
    out["required_by_count"] = (reqs.get("modsRequiringThisMod") or {}).get("totalCount")
    emit(out)


def cmd_top(a, key, opener, sleep):
    if a.trending:
        d = v1_get(f"games/{a.domain}/mods/trending.json", key, opener, sleep)
        mods = [_pick(m, MOD_KEYS) for m in d][: a.n]
        emit({"domain": a.domain, "sort": "trending", "mods": mods})
        return
    data = graphql(
        "query($d:String!,$n:Int){mods(filter:{gameDomainName:[{value:$d,op:EQUALS}]},"
        "sort:[{endorsements:{direction:DESC}}],count:$n){totalCount nodes{modId name"
        " endorsements downloads gameId"
        f" modRequirements{{nexusRequirements(count:50){{nodes{{{REQ_FIELDS}}}}}}}}}}}}}",
        {"d": a.domain, "n": a.n},
        key,
        opener,
        sleep,
    )
    page = data.get("mods") or {}
    mods = []
    for m in page.get("nodes") or []:
        item = _pick(m, ("modId", "name", "endorsements", "downloads"))
        nodes = ((m.get("modRequirements") or {}).get("nexusRequirements") or {}).get("nodes")
        item.update(_split_requirements(nodes, m.get("gameId")))
        mods.append(item)
    emit({"domain": a.domain, "sort": "endorsements", "total": page.get("totalCount"), "mods": mods})


def installers_at_root(paths) -> list[str]:
    return sorted(
        p for p in paths if "/" not in p.strip("/") and p.lower().endswith((".exe", ".msi"))
    )


def cmd_contents(a, key, opener, sleep):
    gid = _game_id(a.domain, key, opener, sleep)
    filt = "gameId:[{value:$g,op:EQUALS}],modId:[{value:$m,op:EQUALS}]"
    variables = {"g": gid, "m": a.mod_id}
    decl = "$g:Int!,$m:Int!"
    if a.file_id is not None:
        filt += ",fileId:[{value:$f,op:EQUALS}]"
        variables["f"] = a.file_id
        decl += ",$f:Int!"
    data = graphql(
        f"query({decl}){{modFileContents(filter:{{{filt}}},count:1000)"
        "{totalCount nodes{fileId filePath fileSize}}}",
        variables,
        key,
        opener,
        sleep,
    )
    page = data.get("modFileContents") or {}
    by_file: dict[int, list[str]] = {}
    for n in page.get("nodes") or []:
        by_file.setdefault(n.get("fileId"), []).append(str(n.get("filePath", "")).replace("\\", "/"))
    files = [
        {
            "file_id": fid,
            "entries": sorted(paths),
            "top_level": sorted({p.strip("/").split("/")[0] for p in paths if p.strip("/")}),
            "top_level_installers": installers_at_root(paths),
        }
        for fid, paths in sorted(by_file.items())
    ]
    emit({"domain": a.domain, "mod_id": a.mod_id, "total_entries": page.get("totalCount"), "files": files})


def cmd_key_status(a, key, opener, sleep):
    raise AssertionError("handled in main")


def _domain(s: str) -> str:
    if not DOMAIN_RE.match(s):
        raise argparse.ArgumentTypeError("domain must match [a-z0-9]+")
    return s


def _pos_int(s: str) -> int:
    if not s.isdigit() or int(s) <= 0:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return int(s)


def _count(s: str) -> int:
    n = _pos_int(s)
    if n > 50:
        raise argparse.ArgumentTypeError("at most 50")
    return n


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="nexus_mod_info.py", description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    for name, fn in (("mod", cmd_mod), ("files", cmd_files), ("requirements", cmd_requirements)):
        s = sub.add_parser(name)
        s.add_argument("domain", type=_domain)
        s.add_argument("mod_id", type=_pos_int)
        s.set_defaults(fn=fn)
    s = sub.add_parser("top")
    s.add_argument("domain", type=_domain)
    s.add_argument("n", type=_count, nargs="?", default=10)
    s.add_argument("--trending", action="store_true", help="v1 trending list (needs a key)")
    s.set_defaults(fn=cmd_top)
    s = sub.add_parser("contents")
    s.add_argument("domain", type=_domain)
    s.add_argument("mod_id", type=_pos_int)
    s.add_argument("file_id", type=_pos_int, nargs="?")
    s.set_defaults(fn=cmd_contents)
    s = sub.add_parser("key-status")
    s.set_defaults(fn=cmd_key_status)
    return p


def main(argv=None, *, opener=None, env=None, keyring=_keyring_lookup, sleep=time.sleep) -> int:
    a = parser().parse_args(argv)
    try:
        key, source = load_key(env, keyring)
        if a.cmd == "key-status":
            emit({"key_found": key is not None, "source": source})
            return 0 if key else 1
        a.fn(a, key, opener, sleep)
        return 0
    except HelperError as e:
        warn(f"error: {e}")
        return e.code
    except (urllib.error.URLError, OSError, ValueError) as e:
        warn(f"error: {type(e).__name__}: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
