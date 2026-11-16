#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""List what a loader archive ships, without extracting or running it.

Usage (always run as `python3 -I`):

  inspect_archive.py github <owner/repo> [--asset GLOB] [--tag TAG]
  inspect_archive.py nexus <domain> <mod_id> <file_id>

Downloads go to ${XDG_CACHE_HOME:-~/.cache}/vessel-game-support/archives/,
over HTTPS only, from an allowlist of hosts (GitHub API, github.com and its
release-asset hosts; the Nexus API and the Nexus CDN hosts its download_link
endpoint returns), up to 200 MiB. The listing is read with zipfile (zip) or
`bsdtar -tf` (7z, rar, tar); nothing is ever extracted or executed.

Nexus download links need a Premium account. Without one, the command exits
3; `nexus_mod_info.py contents <domain> <mod_id> [file_id]` lists the files
from Nexus's own index instead.

Output: one compact JSON object per inspected archive, with `entries`,
`top_level` and `top_level_installers` (.exe/.msi files at the archive root).
"""

from __future__ import annotations

import argparse
import fnmatch
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import zipfile
from pathlib import Path

# `python3 -I` does not put this directory on sys.path; load the sibling
# module by path.
_spec = importlib.util.spec_from_file_location(
    "nexus_mod_info", Path(__file__).resolve().with_name("nexus_mod_info.py")
)
nmi = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(nmi)
HelperError = nmi.HelperError

MAX_BYTES = 200 * 1024 * 1024
MAX_ENTRIES = 2000
MAX_ASSETS = 5
GITHUB_API_HOST = "api.github.com"
GITHUB_HOSTS = frozenset(
    {
        GITHUB_API_HOST,
        "github.com",
        "objects.githubusercontent.com",
        # GitHub now redirects release downloads here.
        "release-assets.githubusercontent.com",
    }
)
NEXUS_CDN_SUFFIXES = (".nexusmods.com", ".nexus-cdn.com")
REPO_RE = re.compile(r"^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$")
TAG_RE = re.compile(r"^[A-Za-z0-9._+/-]{1,200}$")
PREMIUM_MSG = "Nexus download requires Premium; listing unavailable"

MAGIC = (
    (b"PK\x03\x04", "zip"),
    (b"PK\x05\x06", "zip"),
    (b"7z\xbc\xaf\x27\x1c", "7z"),
    (b"Rar!\x1a\x07", "rar"),
    (b"\x1f\x8b", "gzip"),
    (b"\xfd7zXZ\x00", "xz"),
    (b"MZ", "exe"),
)


def cache_dir(env=None) -> Path:
    env = os.environ if env is None else env
    base = env.get("XDG_CACHE_HOME") or os.path.join(
        env.get("HOME") or os.path.expanduser("~"), ".cache"
    )
    return Path(base) / "vessel-game-support" / "archives"


def _safe_name(name: str) -> str:
    name = re.sub(r"[^A-Za-z0-9._+-]", "_", os.path.basename(name)).lstrip(".")
    return name[:200] or "download"


def download(
    url: str,
    dest_dir: Path,
    name: str,
    *,
    allowed_hosts,
    opener=None,
    allow_http: bool = False,
    max_bytes: int = MAX_BYTES,
) -> Path:
    """Streams `url` into dest_dir/name. Refuses non-HTTPS, hosts outside
    `allowed_hosts` (also on redirects) and anything over `max_bytes`.
    `allow_http` exists for tests only; no CLI flag reaches it."""
    nmi.check_url(url, allowed_hosts, allow_http)
    dest_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    dest = dest_dir / _safe_name(name)
    opener = opener or nmi.build_opener(allowed_hosts, allow_http)
    try:
        resp = nmi.request(
            url,
            allowed_hosts=allowed_hosts,
            opener=opener,
            headers={"Accept": "application/octet-stream"},
            allow_http=allow_http,
        )
    except urllib.error.HTTPError as e:
        raise HelperError(f"download failed: HTTP {e.code}") from None
    with resp:
        final = resp.geturl() if hasattr(resp, "geturl") else url
        nmi.check_url(final, allowed_hosts, allow_http)
        length = resp.headers.get("Content-Length")
        if length and length.isdigit() and int(length) > max_bytes:
            raise HelperError(f"refusing download: {int(length)} bytes exceeds the {max_bytes}-byte cap")
        fd, tmp = tempfile.mkstemp(dir=dest_dir, prefix=".part-")
        total = 0
        try:
            with os.fdopen(fd, "wb") as out:
                while chunk := resp.read(1 << 16):
                    total += len(chunk)
                    if total > max_bytes:
                        raise HelperError(f"refusing download: exceeds the {max_bytes}-byte cap")
                    out.write(chunk)
            os.replace(tmp, dest)
        except BaseException:
            Path(tmp).unlink(missing_ok=True)
            raise
    return dest


def detect_format(path: Path) -> str:
    with open(path, "rb") as f:
        head = f.read(512)
    for magic, fmt in MAGIC:
        if head.startswith(magic):
            return fmt
    if len(head) >= 262 and head[257:262] == b"ustar":
        return "tar"
    return "unknown"


def list_archive(path: Path, which=shutil.which) -> tuple[str, list[str]]:
    """Returns (format, entry paths). Reads only the archive index."""
    fmt = detect_format(path)
    if fmt == "zip":
        try:
            with zipfile.ZipFile(path) as z:
                return fmt, [i.filename for i in z.infolist()]
        except zipfile.BadZipFile as e:
            raise HelperError(f"unreadable zip: {e}") from None
    if fmt == "exe":
        return fmt, []
    if fmt in ("7z", "rar", "gzip", "xz", "tar"):
        bsdtar = which("bsdtar")
        if not bsdtar:
            raise HelperError(f"unsupported format: {fmt} (install bsdtar/libarchive to list it)")
        r = subprocess.run(
            [bsdtar, "-tf", str(path)],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=120,
        )
        if r.returncode != 0:
            raise HelperError(f"bsdtar could not list the {fmt} archive: {r.stderr.strip()[:300]}")
        return fmt, [ln for ln in r.stdout.splitlines() if ln]
    raise HelperError("unsupported format: not a zip, 7z, rar or tar archive")


def describe(path: Path, source: str, which=shutil.which) -> dict:
    fmt, raw = list_archive(path, which)
    entries = [e.replace("\\", "/") for e in raw]
    out = {
        "source": source,
        "file": str(path),
        "bytes": path.stat().st_size,
        "format": fmt,
        "entry_count": len(entries),
        "entries": entries[:MAX_ENTRIES],
        "truncated": len(entries) > MAX_ENTRIES,
        "top_level": sorted({e.strip("/").split("/")[0] for e in entries if e.strip("/")}),
        "top_level_installers": nmi.installers_at_root(entries),
    }
    if fmt == "exe":
        out["note"] = "the download itself is a Windows executable (installer), not an archive"
        out["top_level_installers"] = [path.name]
    out["top_level_installer"] = bool(out["top_level_installers"])
    return out


# ---- sources ---------------------------------------------------------------


def _github_json(url: str, opener):
    try:
        resp = nmi.request(
            url,
            allowed_hosts=GITHUB_HOSTS,
            opener=opener,
            headers={"Accept": "application/vnd.github+json"},
        )
    except urllib.error.HTTPError as e:
        raise HelperError(f"GitHub API: HTTP {e.code} for {urllib.parse.urlsplit(url).path}") from None
    with resp:
        return json.loads(resp.read().decode("utf-8"))


def cmd_github(a, *, opener, env, which, key_loader) -> int:
    if not REPO_RE.match(a.repo):
        raise HelperError("repo must be <owner>/<repo>")
    base = f"https://{GITHUB_API_HOST}/repos/{a.repo}/releases"
    if a.tag:
        if not TAG_RE.match(a.tag):
            raise HelperError("invalid --tag")
        url = f"{base}/tags/{urllib.parse.quote(a.tag, safe='')}"
    else:
        url = f"{base}/latest"
    rel = _github_json(url, opener)
    assets = rel.get("assets") or []
    names = [x.get("name") for x in assets]
    if a.asset:
        chosen = [x for x in assets if fnmatch.fnmatchcase(x.get("name") or "", a.asset)]
    else:
        chosen = assets if len(assets) == 1 else []
    if not chosen:
        nmi.emit({"repo": a.repo, "tag": rel.get("tag_name"), "assets": names})
        raise HelperError(
            "no asset selected: pass --asset GLOB matching one of the assets listed above", code=2
        )
    if len(chosen) > MAX_ASSETS:
        raise HelperError(f"--asset matches {len(chosen)} assets; narrow it to at most {MAX_ASSETS}", code=2)
    dest = cache_dir(env) / "github" / a.repo.replace("/", "_") / _safe_name(rel.get("tag_name") or "latest")
    for x in chosen:
        size = x.get("size")
        if isinstance(size, int) and size > MAX_BYTES:
            raise HelperError(f"refusing {x.get('name')}: {size} bytes exceeds the {MAX_BYTES}-byte cap")
        target = dest / _safe_name(x["name"])
        if not (target.is_file() and target.stat().st_size == size):
            target = download(
                x["browser_download_url"], dest, x["name"], allowed_hosts=GITHUB_HOSTS, opener=opener
            )
        out = describe(target, f"github {a.repo} {rel.get('tag_name')} {x['name']}", which)
        nmi.emit(out)
    return 0


def _cdn_ok(url: str) -> bool:
    p = urllib.parse.urlsplit(url)
    host = (p.hostname or "").lower()
    return p.scheme == "https" and host.endswith(NEXUS_CDN_SUFFIXES)


def cmd_nexus(a, *, opener, env, which, key_loader) -> int:
    key, _source = key_loader(env)
    if not key:
        raise HelperError("no Nexus API key found; cannot request a Nexus download link")
    path = f"games/{a.domain}/mods/{a.mod_id}/files/{a.file_id}/download_link.json"
    try:
        resp = nmi.request(
            f"{nmi.API}/v1/{path}", allowed_hosts={nmi.API_HOST}, opener=opener, key=key
        )
        with resp:
            links = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        if e.code == 403:
            raise HelperError(
                f"{PREMIUM_MSG} (for Nexus's own file index, run: nexus_mod_info.py contents "
                f"{a.domain} {a.mod_id} {a.file_id})",
                code=3,
            ) from None
        raise HelperError(f"GET /v1/{path}: HTTP {e.code}") from None
    uris = [x.get("URI") for x in links if isinstance(x, dict) and isinstance(x.get("URI"), str)]
    uris = [u for u in uris if _cdn_ok(u)]
    if not uris:
        raise HelperError("download_link returned no HTTPS Nexus CDN link")
    allowed = {nmi.API_HOST} | {urllib.parse.urlsplit(u).hostname.lower() for u in uris}
    name = urllib.parse.unquote(os.path.basename(urllib.parse.urlsplit(uris[0]).path))
    dest = cache_dir(env) / "nexus" / a.domain / str(a.mod_id) / str(a.file_id)
    target = download(uris[0], dest, name, allowed_hosts=allowed, opener=opener)
    nmi.emit(describe(target, f"nexus {a.domain} {a.mod_id} {a.file_id}", which))
    return 0


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="inspect_archive.py", description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("github", help="a GitHub release asset")
    g.add_argument("repo", metavar="owner/repo")
    g.add_argument("--asset", metavar="GLOB")
    g.add_argument("--tag")
    g.set_defaults(fn=cmd_github)
    n = sub.add_parser("nexus", help="a Nexus file (needs Premium)")
    n.add_argument("domain", type=nmi._domain)
    n.add_argument("mod_id", type=nmi._pos_int)
    n.add_argument("file_id", type=nmi._pos_int)
    n.set_defaults(fn=cmd_nexus)
    return p


def main(argv=None, *, opener=None, env=None, which=shutil.which, key_loader=None) -> int:
    a = parser().parse_args(argv)
    key_loader = key_loader or (lambda e: nmi.load_key(e))
    try:
        return a.fn(a, opener=opener, env=env, which=which, key_loader=key_loader)
    except HelperError as e:
        nmi.warn(f"error: {e}")
        return e.code
    except (urllib.error.URLError, OSError, ValueError, subprocess.SubprocessError) as e:
        nmi.warn(f"error: {type(e).__name__}: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
