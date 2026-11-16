#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for inspect_archive.py: host allowlist, HTTPS-only, size cap,
archive listing / installer detection and the Nexus Premium 403 path.
Downloads use a local http.server (with the test-only HTTP override) or an
injected opener.

Run: python3 -B -m unittest scripts/test_inspect_archive.py
"""

from __future__ import annotations

import contextlib
import http.server
import importlib.util
import io
import json
import os
import tempfile
import threading
import unittest
import zipfile
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "inspect_archive", Path(__file__).with_name("inspect_archive.py")
)
ia = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ia)
nmi = ia.nmi

_tspec = importlib.util.spec_from_file_location(
    "test_nexus_mod_info", Path(__file__).with_name("test_nexus_mod_info.py")
)
tn = importlib.util.module_from_spec(_tspec)
_tspec.loader.exec_module(tn)
FakeOpener, headers = tn.FakeOpener, tn.headers

KEY = "SECRETKEYxyz789=="
LOCAL = {"127.0.0.1"}


def zip_bytes(names) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        for n in names:
            z.writestr(n, b"x")
    return buf.getvalue()


class Handler(http.server.BaseHTTPRequestHandler):
    body = b"A" * 2048

    def log_message(self, *a):
        pass

    def do_GET(self):
        if self.path == "/redirect-evil":
            self.send_response(302)
            self.send_header("Location", "http://evil.invalid/x")
            self.end_headers()
        elif self.path == "/nolength":
            # No Content-Length: the cap must hold while streaming.
            self.send_response(200)
            self.end_headers()
            self.wfile.write(self.body)
            self.close_connection = True
        else:
            self.send_response(200)
            self.send_header("Content-Length", str(len(self.body)))
            self.end_headers()
            self.wfile.write(self.body)


class Downloads(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.base = f"http://127.0.0.1:{cls.srv.server_address[1]}"
        threading.Thread(target=cls.srv.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()
        cls.srv.server_close()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def dl(self, path, **kw):
        kw.setdefault("allowed_hosts", LOCAL)
        kw.setdefault("allow_http", True)
        with contextlib.redirect_stderr(io.StringIO()):
            return ia.download(self.base + path, self.dir, "a.zip", **kw)

    def test_within_cap_downloads(self):
        self.assertEqual(self.dl("/ok").read_bytes(), Handler.body)

    def test_size_cap_by_content_length_and_streaming(self):
        # Catches: oversized downloads written to disk, with or without a
        # Content-Length header, and partial files left behind.
        for path in ("/ok", "/nolength"):
            with self.subTest(path=path), self.assertRaisesRegex(ia.HelperError, "cap"):
                self.dl(path, max_bytes=1000)
            self.assertEqual(list(self.dir.iterdir()), [])

    def test_non_https_rejected_by_default(self):
        with self.assertRaisesRegex(ia.HelperError, "non-HTTPS"):
            ia.download(self.base + "/ok", self.dir, "a.zip", allowed_hosts=LOCAL)

    def test_http_override_not_on_cli(self):
        # Catches: the test-only HTTP override being exposed as a flag.
        for argv in (["github", "a/b", "--allow-http"], ["nexus", "d", "1", "2", "--allow-http"]):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                ia.main(argv)
        self.assertNotIn("allow", ia.parser().format_help().lower())

    def test_host_not_in_allowlist_rejected(self):
        with self.assertRaisesRegex(ia.HelperError, "allowlist"):
            ia.download("https://evil.example/x.zip", self.dir, "a.zip", allowed_hosts=ia.GITHUB_HOSTS)

    def test_redirect_to_other_host_rejected(self):
        # Catches: an allowed host redirecting the download elsewhere.
        with self.assertRaisesRegex(ia.HelperError, "allowlist"):
            self.dl("/redirect-evil")
        self.assertEqual(list(self.dir.iterdir()), [])


class Listing(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def file(self, name, data: bytes) -> Path:
        p = self.dir / name
        p.write_bytes(data)
        return p

    def test_zip_listing_and_root_installer(self):
        # Catches: a nested .exe reported as an installer route, or a root
        # installer missed.
        p = self.file("a.zip", zip_bytes(["Setup.EXE", "tools/helper.exe", "Mods/a.dll"]))
        d = ia.describe(p, "t")
        self.assertEqual(d["format"], "zip")
        self.assertEqual(d["top_level_installers"], ["Setup.EXE"])
        self.assertTrue(d["top_level_installer"])
        self.assertEqual(d["top_level"], ["Mods", "Setup.EXE", "tools"])

    def test_loader_zip_has_no_installer(self):
        p = self.file("ml.zip", zip_bytes(["version.dll", "MelonLoader/net6/MelonLoader.dll"]))
        d = ia.describe(p, "t")
        self.assertEqual(d["top_level"], ["MelonLoader", "version.dll"])
        self.assertFalse(d["top_level_installer"])

    def test_msi_at_root_detected(self):
        d = ia.describe(self.file("b.zip", zip_bytes(["pkg.msi"])), "t")
        self.assertEqual(d["top_level_installers"], ["pkg.msi"])

    def test_bare_executable(self):
        d = ia.describe(self.file("Installer.exe", b"MZ" + b"\0" * 100), "t")
        self.assertEqual((d["format"], d["top_level_installer"]), ("exe", True))

    def test_7z_without_bsdtar_is_unsupported(self):
        p = self.file("a.7z", b"7z\xbc\xaf\x27\x1c" + b"\0" * 32)
        with self.assertRaisesRegex(ia.HelperError, "unsupported format: 7z"):
            ia.list_archive(p, which=lambda name: None)

    def test_unknown_format(self):
        with self.assertRaisesRegex(ia.HelperError, "unsupported format"):
            ia.list_archive(self.file("x.bin", b"hello"))


def run_main(argv, **kw):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        rc = ia.main(argv, **kw)
    return rc, out.getvalue(), err.getvalue()


class Sources(unittest.TestCase):
    def setUp(self):
        nmi._SECRETS.clear()
        self.tmp = tempfile.TemporaryDirectory()
        self.env = {"XDG_CACHE_HOME": self.tmp.name}
        self.keyed = lambda env: (nmi._remember(KEY), "test")

    def tearDown(self):
        self.tmp.cleanup()

    def test_nexus_403_premium_message(self):
        # Catches: a crash/traceback, or exit 0, for non-Premium accounts.
        opener = FakeOpener((403, b'{"message":"premium only"}', headers()))
        rc, out, err = run_main(["nexus", "schedule1", "1196", "3486"], opener=opener,
                                env=self.env, key_loader=self.keyed)
        self.assertEqual(rc, 3)
        self.assertIn(ia.PREMIUM_MSG, err)
        self.assertNotIn("Traceback", err)
        self.assertEqual(out, "")
        self.assertNotIn(KEY, out + err)

    def test_nexus_cdn_download_listed_without_key(self):
        # Catches: the API key forwarded to the CDN, or a non-Nexus link used.
        links = [{"name": "evil", "URI": "https://evil.example/a.zip"},
                 {"name": "CDN", "URI": "https://cf-files.nexusmods.com/x/MelonLoader-1196.zip?md5=1"}]
        opener = FakeOpener(links, zip_bytes(["MelonLoader.Installer.exe"]))
        rc, out, err = run_main(["nexus", "schedule1", "1196", "3486"], opener=opener,
                                env=self.env, key_loader=self.keyed)
        self.assertEqual(rc, 0, err)
        self.assertEqual(opener.requests[1].host, "cf-files.nexusmods.com")
        self.assertNotIn("Apikey", opener.requests[1].unredirected_hdrs)
        self.assertNotIn("Apikey", opener.requests[1].headers)
        d = json.loads(out)
        self.assertEqual(d["top_level_installers"], ["MelonLoader.Installer.exe"])
        self.assertTrue(d["file"].startswith(self.tmp.name))
        self.assertNotIn(KEY, out + err)

    def test_nexus_without_key(self):
        rc, _, err = run_main(["nexus", "schedule1", "1", "2"], opener=FakeOpener(),
                              env=self.env, key_loader=lambda env: (None, "none"))
        self.assertEqual(rc, 1)
        self.assertIn("no Nexus API key", err)

    def test_github_release_asset(self):
        rel = {"tag_name": "v0.7.3", "assets": [
            {"name": "MelonLoader.Installer.exe", "size": 10,
             "browser_download_url": "https://github.com/o/r/releases/download/v/i.exe"},
            {"name": "MelonLoader.x64.zip", "size": 1,
             "browser_download_url": "https://github.com/o/r/releases/download/v/MelonLoader.x64.zip"}]}
        opener = FakeOpener(rel, zip_bytes(["version.dll", "MelonLoader/a.dll"]))
        rc, out, err = run_main(["github", "LavaGang/MelonLoader", "--asset", "*.x64.zip"],
                                opener=opener, env=self.env)
        self.assertEqual(rc, 0, err)
        self.assertTrue(opener.requests[0].full_url.endswith("/repos/LavaGang/MelonLoader/releases/latest"))
        d = json.loads(out)
        self.assertEqual(d["top_level"], ["MelonLoader", "version.dll"])
        self.assertTrue(d["file"].startswith(os.path.join(self.tmp.name, "vessel-game-support", "archives")))

    def test_github_asset_off_allowlist_rejected(self):
        rel = {"tag_name": "v1", "assets": [
            {"name": "a.zip", "size": 1, "browser_download_url": "https://evil.example/a.zip"}]}
        rc, _, err = run_main(["github", "o/r"], opener=FakeOpener(rel), env=self.env)
        self.assertEqual(rc, 1)
        self.assertIn("allowlist", err)

    def test_github_ambiguous_assets_lists_names(self):
        rel = {"tag_name": "v1", "assets": [{"name": "a.zip"}, {"name": "b.zip"}]}
        rc, out, err = run_main(["github", "o/r"], opener=FakeOpener(rel), env=self.env)
        self.assertEqual(rc, 2)
        self.assertEqual(json.loads(out)["assets"], ["a.zip", "b.zip"])
        self.assertIn("--asset", err)


if __name__ == "__main__":
    unittest.main()
