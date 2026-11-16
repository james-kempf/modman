#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for nexus_mod_info.py: key sources, key secrecy, 429 retry, URL
policy and requirement parsing. HTTP is faked by injecting an opener.

Run: python3 -B -m unittest scripts/test_nexus_mod_info.py
"""

from __future__ import annotations

import contextlib
import email.message
import importlib.util
import io
import json
import os
import tempfile
import unittest
import urllib.error
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "nexus_mod_info", Path(__file__).with_name("nexus_mod_info.py")
)
nmi = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(nmi)

KEY = "SECRETKEYabc123=="


def headers(**kv) -> email.message.Message:
    m = email.message.Message()
    for k, v in kv.items():
        m[k.replace("_", "-")] = v
    return m


class FakeResp(io.BytesIO):
    def __init__(self, body: bytes, url: str, hdrs=None):
        super().__init__(body)
        self.headers = hdrs or headers()
        self.url = url
        self.status = 200

    def geturl(self):
        return self.url


class FakeOpener:
    """Replays queued responses (bytes/dict -> 200, int -> HTTPError) and
    records each request."""

    def __init__(self, *responses, hdrs=None):
        self.responses = list(responses)
        self.requests = []
        self.hdrs = hdrs

    def open(self, req, timeout=None):
        self.requests.append(req)
        r = self.responses.pop(0)
        if isinstance(r, tuple):
            code, body, h = r
            raise urllib.error.HTTPError(req.full_url, code, "err", h, io.BytesIO(body))
        if isinstance(r, (dict, list)):
            r = json.dumps(r).encode()
        return FakeResp(r, req.full_url, self.hdrs)


def run_main(argv, **kw):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        rc = nmi.main(argv, **kw)
    return rc, out.getvalue(), err.getvalue()


class KeySources(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.env = {"XDG_CONFIG_HOME": self.tmp.name, "HOME": self.tmp.name}
        self.warnings = []

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, path: str, text: str, mode: int):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        Path(path).write_text(text)
        os.chmod(path, mode)

    def load(self, keyring=lambda: None):
        return nmi.load_key(self.env, keyring, self.warnings.append)

    def test_key_file_with_loose_mode_warns_but_is_used(self):
        # Catches: a world-readable key file silently accepted.
        self.write(nmi.key_file_path(self.env), KEY + "\n", 0o644)
        key, source = self.load()
        self.assertEqual(key, KEY)
        self.assertIn("key file", source)
        self.assertTrue(any("0644" in w and "0600" in w for w in self.warnings), self.warnings)
        self.assertFalse(any(KEY in w for w in self.warnings))

    def test_key_file_0600_no_warning(self):
        self.write(nmi.key_file_path(self.env), KEY, 0o600)
        self.assertEqual(self.load()[0], KEY)
        self.assertEqual(self.warnings, [])

    def test_vessel_keyring_wins(self):
        # Catches: the helper-only file shadowing Vessel's own stored key.
        self.write(nmi.key_file_path(self.env), "OTHERKEY123", 0o600)
        key, source = self.load(keyring=lambda: KEY + "\n")
        self.assertEqual((key, source), (KEY, "vessel keyring"))

    def test_vessel_credentials_file_before_key_file(self):
        self.write(nmi.vessel_credentials_path(self.env), f'nexus_api_key = "{KEY}"\n', 0o600)
        self.write(nmi.key_file_path(self.env), "OTHERKEY123", 0o600)
        key, source = self.load()
        self.assertEqual(key, KEY)
        self.assertIn("credentials.toml", source)

    def test_loose_vessel_credentials_refused_like_vessel(self):
        self.write(nmi.vessel_credentials_path(self.env), f'nexus_api_key = "{KEY}"\n', 0o644)
        self.assertEqual(self.load(), (None, "none"))
        self.assertTrue(any("ignoring" in w for w in self.warnings))

    def test_malformed_key_with_newline_ignored(self):
        # Catches: header injection through a key containing CR/LF.
        self.write(nmi.key_file_path(self.env), "abc\r\nX-Evil: 1", 0o600)
        self.assertEqual(self.load()[0], None)

    def test_bad_toml_is_not_echoed(self):
        self.write(nmi.vessel_credentials_path(self.env), f'nexus_api_key = "{KEY}\n', 0o600)
        self.load()
        self.assertFalse(any(KEY in w for w in self.warnings))


class KeyNeverPrinted(unittest.TestCase):
    def setUp(self):
        nmi._SECRETS.clear()
        self.kw = {"env": {"XDG_CONFIG_HOME": "/nonexistent", "HOME": "/nonexistent"},
                   "keyring": lambda: KEY, "sleep": lambda s: None}

    def test_success_error_and_status_paths(self):
        # Catches: the key leaking via output, error bodies echoing it, or
        # being sent as a redirectable header.
        echo = {"mod_id": 1, "name": "x", "summary": f"apikey={KEY}", "description": KEY}
        opener = FakeOpener(echo)
        rc, out, err = run_main(["mod", "schedule1", "1"], opener=opener, **self.kw)
        self.assertEqual(rc, 0)
        req = opener.requests[0]
        self.assertEqual(req.unredirected_hdrs.get("Apikey"), KEY)
        self.assertNotIn("Apikey", req.headers)

        opener = FakeOpener((401, f'{{"message":"bad key {KEY}"}}'.encode(), headers()))
        rc2, out2, err2 = run_main(["files", "schedule1", "1"], opener=opener, **self.kw)
        self.assertEqual(rc2, 1)
        self.assertIn("401", err2)

        rc3, out3, err3 = run_main(["key-status"], **self.kw)
        self.assertEqual(rc3, 0)
        self.assertEqual(json.loads(out3)["source"], "vessel keyring")
        for text in (out, err, out2, err2, out3, err3):
            self.assertNotIn(KEY, text)
        self.assertIn("<redacted>", out)

    def test_key_status_without_key_exits_1(self):
        kw = dict(self.kw, keyring=lambda: None)
        rc, out, _ = run_main(["key-status"], **kw)
        self.assertEqual((rc, json.loads(out)["key_found"]), (1, False))


class Http(unittest.TestCase):
    def test_429_retries_and_reports_rate_limit(self):
        # Catches: a single 429 aborting the research, and missing quota info.
        rl = headers(X_RL_Hourly_Remaining="0", X_RL_Daily_Remaining="10", Retry_After="1")
        opener = FakeOpener((429, b"", rl), {"ok": True},
                            hdrs=headers(X_RL_Hourly_Remaining="99"))
        slept = []
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            resp = nmi.request(f"{nmi.API}/v1/x", allowed_hosts={nmi.API_HOST},
                               opener=opener, sleep=slept.append)
        self.assertEqual(json.loads(resp.read()), {"ok": True})
        self.assertEqual(slept, [1.0])
        self.assertIn("hourly-remaining=0", err.getvalue())
        self.assertIn("hourly-remaining=99", err.getvalue())

    def test_429_gives_up_after_max_tries(self):
        opener = FakeOpener(*[(429, b"", headers())] * nmi.MAX_TRIES)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(urllib.error.HTTPError):
            nmi.request(f"{nmi.API}/v1/x", allowed_hosts={nmi.API_HOST},
                        opener=opener, sleep=lambda s: None)
        self.assertEqual(len(opener.requests), nmi.MAX_TRIES)

    def test_url_policy(self):
        with self.assertRaisesRegex(nmi.HelperError, "non-HTTPS"):
            nmi.check_url("http://api.nexusmods.com/v1", {nmi.API_HOST})
        with self.assertRaisesRegex(nmi.HelperError, "allowlist"):
            nmi.check_url("https://evil.example/v1", {nmi.API_HOST})

    def test_key_only_sent_to_api_host(self):
        # Catches: the key reaching a CDN or GitHub host.
        with self.assertRaisesRegex(nmi.HelperError, "non-API host"):
            nmi.request("https://cf-files.nexusmods.com/x", allowed_hosts={"cf-files.nexusmods.com"},
                        opener=FakeOpener(b""), key=KEY)

    def test_v1_without_key_is_a_clean_error(self):
        rc, _, err = run_main(["mod", "schedule1", "1"], env={"XDG_CONFIG_HOME": "/nonexistent"},
                              keyring=lambda: None)
        self.assertEqual(rc, 1)
        self.assertIn("no Nexus API key", err)

    def test_cli_rejects_bad_domain(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            nmi.main(["mod", "../x", "1"], keyring=lambda: None)


class Requirements(unittest.TestCase):
    def test_splits_nexus_offsite_and_other_domain(self):
        # Catches: off-site (GitHub/wiki) requirements lost or mixed into
        # Nexus ones, and cross-domain requirements not flagged.
        game = {"data": {"game": {"id": 7381}}}
        mod = {"data": {"mod": {"name": "M", "modRequirements": {
            "nexusRequirements": {"nodes": [
                {"modId": "0", "modName": "MelonLoader", "gameId": "0",
                 "url": "https://github.com/LavaGang/MelonLoader", "externalRequirement": True},
                {"modId": "1196", "modName": "ML", "gameId": "7381", "url": "u",
                 "externalRequirement": False},
                {"modId": "5", "modName": "Other", "gameId": "2295", "url": "u2",
                 "externalRequirement": False},
            ]},
            "modsRequiringThisMod": {"totalCount": 3}}}}}
        rc, out, _ = run_main(["requirements", "schedule1", "9"], opener=FakeOpener(game, mod),
                              env={"XDG_CONFIG_HOME": "/nonexistent"}, keyring=lambda: None)
        self.assertEqual(rc, 0)
        d = json.loads(out)
        self.assertEqual([r["modName"] for r in d["offsite_requirements"]], ["MelonLoader"])
        self.assertEqual([r["modId"] for r in d["nexus_requirements"]], ["1196", "5"])
        self.assertNotIn("other_game_id", d["nexus_requirements"][0])
        self.assertEqual(d["nexus_requirements"][1]["other_game_id"], "2295")
        self.assertEqual(d["required_by_count"], 3)

    def test_graphql_errors_are_reported(self):
        opener = FakeOpener({"errors": [{"message": "boom"}]})
        rc, _, err = run_main(["requirements", "schedule1", "9"], opener=opener,
                              env={"XDG_CONFIG_HOME": "/nonexistent"}, keyring=lambda: None)
        self.assertEqual(rc, 1)
        self.assertIn("GraphQL error: boom", err)


if __name__ == "__main__":
    unittest.main()
