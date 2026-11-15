#!/usr/bin/env python3
"""modman web UI server — Python standard library only (no Node needed at runtime).

Serves the built frontend (web/dist) and a small JSON API. Every action runs the
modman script itself, so the CLI stays the single source of truth.

Security: listens on 127.0.0.1 only, rejects foreign Host headers (DNS rebinding)
and requires a per-launch token that is only embedded in the served page.
"""
import argparse
import json
import mimetypes
import os
import re
import secrets
import signal
import socket
import subprocess
import sys
import threading
import time
import webbrowser
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

ANSI_RE = re.compile(r'\x1b\[[0-9;?]*[ -/]*[@-~]')
DOMAIN_RE = re.compile(r'^[a-z0-9]{1,64}$')
MAX_JOBS = 40
MAX_LINES = 8000

# Commands the UI may run, with the sub-commands allowed for each (None = any arguments).
ALLOWED = {
    'install': None, 'uninstall': None, 'reinstall': None, 'enable': None, 'disable': None,
    'order': None, 'deploy': None, 'purge': None, 'verify': None, 'updates': None,
    'clean': None, 'launch': None, 'logout': None, 'login': None,
    'collection': {'install', 'update', 'remove'},
    'deps': {'ack', 'unack'},
    'autoupdate': {'enable', 'disable', 'run'},
    'game': {'use', 'set', 'add', 'remove'},
    'config': {'deploy-method', 'jobs', 'cdn', 'adult', 'auto-deploy', 'fomod',
               'autoupdate-collections', 'autoupdate-interval'},
    'nxm': {'register', 'unregister'},
    'ui': {'desktop'},
}
# Commands that only read: run them synchronously and return their JSON output.
QUERIES = {'state': ['ui-state'], 'detect': ['ui-detect'], 'thumbs': ['ui-thumbs']}


class Job:
    def __init__(self, jid, args, game, label):
        self.id = jid
        self.args = args
        self.game = game
        self.label = label
        self.lines = []
        self.status = 'running'
        self.code = None
        self.started = time.time()
        self.ended = None
        self.proc = None
        self.cond = threading.Condition()

    def summary(self):
        return {'id': self.id, 'args': self.args, 'game': self.game, 'label': self.label,
                'status': self.status, 'code': self.code, 'started': self.started,
                'ended': self.ended, 'lines': len(self.lines),
                'last': self.lines[-1] if self.lines else ''}


class Jobs:
    def __init__(self, modman):
        self.modman = modman
        self.jobs = {}
        self.order = []
        self.lock = threading.Lock()
        self.seq = 0

    def busy(self):
        with self.lock:
            return any(j.status == 'running' for j in self.jobs.values())

    def start(self, args, game, stdin_text, label):
        with self.lock:
            if any(j.status == 'running' for j in self.jobs.values()):
                return None
            self.seq += 1
            job = Job('%d-%s' % (self.seq, secrets.token_hex(3)), args, game, label)
            self.jobs[job.id] = job
            self.order.append(job.id)
            while len(self.order) > MAX_JOBS:
                old = self.order.pop(0)
                if self.jobs[old].status != 'running':
                    del self.jobs[old]
        cmd = [self.modman, '--no-color', '-y']
        if game:
            cmd += ['-g', game]
        cmd += args
        env = dict(os.environ, TERM='dumb', COLUMNS='110', MODMAN_NO_COLOR='1')
        try:
            job.proc = subprocess.Popen(
                cmd, stdin=subprocess.PIPE if stdin_text is not None else subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, start_new_session=True)
        except OSError as e:
            self._finish(job, 127, ['Could not start modman: %s' % e])
            return job
        if stdin_text is not None:
            try:
                job.proc.stdin.write(stdin_text.encode() + b'\n')
                job.proc.stdin.close()
            except OSError:
                pass
        threading.Thread(target=self._pump, args=(job,), daemon=True).start()
        return job

    def _pump(self, job):
        buf = b''
        current = ''
        stream = job.proc.stdout
        while True:
            chunk = stream.read1(4096) if hasattr(stream, 'read1') else stream.read(4096)
            if not chunk:
                break
            buf += chunk
            while b'\n' in buf:
                raw, buf = buf.split(b'\n', 1)
                text = ANSI_RE.sub('', raw.decode('utf-8', 'replace'))
                text = text.split('\r')[-1]          # progress bars redraw with \r: keep the last state
                self._add(job, current + text)
                current = ''
        if buf:
            self._add(job, ANSI_RE.sub('', buf.decode('utf-8', 'replace')).split('\r')[-1])
        code = job.proc.wait()
        self._finish(job, code, [])

    def _add(self, job, line):
        with job.cond:
            job.lines.append(line.rstrip())
            if len(job.lines) > MAX_LINES:
                del job.lines[:len(job.lines) - MAX_LINES]
            job.cond.notify_all()

    def _finish(self, job, code, extra):
        with job.cond:
            job.lines.extend(extra)
            job.code = code
            job.status = 'done' if code == 0 else ('cancelled' if code in (130, -15, 143) else 'failed')
            job.ended = time.time()
            job.cond.notify_all()

    def cancel(self, jid):
        job = self.jobs.get(jid)
        if job and job.status == 'running' and job.proc:
            try:
                os.killpg(job.proc.pid, signal.SIGTERM)
            except OSError:
                pass
            return True
        return False


def run_query(modman, args, game, timeout=60):
    cmd = [modman, '--no-color', '-y']
    if game:
        cmd += ['-g', game]
    cmd += args
    env = dict(os.environ, TERM='dumb', MODMAN_NO_COLOR='1')
    p = subprocess.run(cmd, stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout, env=env)
    out = p.stdout.decode('utf-8', 'replace').strip()
    if p.returncode != 0:
        err = ANSI_RE.sub('', (p.stderr or p.stdout).decode('utf-8', 'replace')).strip()
        raise RuntimeError(err.splitlines()[-1] if err else 'modman exited with %d' % p.returncode)
    return out


def make_handler(cfg):
    static_root = Path(cfg.static).resolve()

    def index_html():   # read per request so a rebuilt frontend is picked up without a restart
        return (static_root / 'index.html').read_text(encoding='utf-8').replace('__MODMAN_TOKEN__', cfg.token)
    jobs = cfg.jobs
    allowed_hosts = {'127.0.0.1:%d' % cfg.port, 'localhost:%d' % cfg.port, '[::1]:%d' % cfg.port}
    log_file = Path(os.environ.get('MODMAN_CONFIG_DIR') or Path.home() / '.config/nexus-mod-manager') / 'nmm.log'

    class Handler(BaseHTTPRequestHandler):
        server_version = 'modman-ui'
        protocol_version = 'HTTP/1.1'

        def log_message(self, fmt, *a):
            if cfg.verbose:
                sys.stderr.write('[ui] ' + (fmt % a) + '\n')

        # ── helpers ──────────────────────────────────────
        def _host_ok(self):
            return self.headers.get('Host', '') in allowed_hosts

        def _send(self, status, body, ctype='application/json; charset=utf-8', extra=None):
            data = body if isinstance(body, bytes) else body.encode('utf-8')
            self.send_response(status)
            self.send_header('Content-Type', ctype)
            self.send_header('Content-Length', str(len(data)))
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Referrer-Policy', 'no-referrer')
            for k, v in (extra or {}).items():
                self.send_header(k, v)
            self.end_headers()
            if self.command != 'HEAD':
                self.wfile.write(data)

        def _json(self, obj, status=200):
            self._send(status, json.dumps(obj))

        def _error(self, status, msg):
            self._json({'error': msg}, status)

        def _body(self):
            n = int(self.headers.get('Content-Length') or 0)
            if n > 65536:
                raise ValueError('request too large')
            raw = self.rfile.read(n) if n else b'{}'
            return json.loads(raw or b'{}')

        def _game(self, value):
            if value in (None, ''):
                return None
            if not DOMAIN_RE.match(value):
                raise ValueError('invalid game')
            return value

        # ── routing ──────────────────────────────────────
        def do_GET(self):
            if not self._host_ok():
                return self._error(HTTPStatus.FORBIDDEN, 'bad host')
            url = urlparse(self.path)
            if url.path.startswith('/api/'):
                if not secrets.compare_digest(self.headers.get('X-Modman-Token', ''), cfg.token):
                    return self._error(HTTPStatus.FORBIDDEN, 'missing or wrong token')
                try:
                    return self._api_get(url.path, parse_qs(url.query))
                except ValueError as e:
                    return self._error(400, str(e))
                except RuntimeError as e:
                    return self._error(502, str(e))
                except subprocess.TimeoutExpired:
                    return self._error(504, 'modman did not answer in time')
            return self._static(url.path)

        do_HEAD = do_GET

        def do_POST(self):
            if not self._host_ok():
                return self._error(HTTPStatus.FORBIDDEN, 'bad host')
            if not secrets.compare_digest(self.headers.get('X-Modman-Token', ''), cfg.token):
                return self._error(HTTPStatus.FORBIDDEN, 'missing or wrong token')
            url = urlparse(self.path)
            try:
                body = self._body()
                if url.path == '/api/jobs':
                    return self._start_job(body)
                m = re.match(r'^/api/jobs/([\w-]+)/cancel$', url.path)
                if m:
                    return self._json({'cancelled': jobs.cancel(m.group(1))})
            except (ValueError, json.JSONDecodeError) as e:
                return self._error(400, str(e))
            return self._error(404, 'not found')

        def _api_get(self, path, q):
            game = self._game((q.get('game') or [None])[0])
            name = path[len('/api/'):]
            if name in QUERIES:
                out = run_query(cfg.modman, QUERIES[name], game)
                return self._send(200, out or 'null')
            if name == 'deps':
                args = ['deps', '--json'] + (['--refresh'] if (q.get('refresh') or ['0'])[0] == '1' else [])
                return self._send(200, run_query(cfg.modman, args, game, timeout=180) or '[]')
            if name == 'search':
                text = ((q.get('q') or [''])[0]).strip()[:120]
                if not text:
                    raise ValueError('empty search')
                return self._send(200, run_query(cfg.modman, ['search', '--json', text], game))
            if name == 'log':
                n = min(int((q.get('lines') or ['300'])[0]), 3000)
                try:
                    with open(log_file, 'rb') as fh:
                        fh.seek(0, 2)
                        size = fh.tell()
                        fh.seek(max(0, size - 400_000))
                        lines = fh.read().decode('utf-8', 'replace').splitlines()[-n:]
                except OSError:
                    lines = []
                return self._json({'lines': lines, 'file': str(log_file)})
            if name == 'jobs':
                with jobs.lock:
                    lst = [jobs.jobs[i].summary() for i in reversed(jobs.order) if i in jobs.jobs]
                return self._json({'jobs': lst, 'busy': any(j['status'] == 'running' for j in lst)})
            m = re.match(r'^jobs/([\w-]+)/events$', name)
            if m:
                return self._events(m.group(1), int((q.get('from') or ['0'])[0]))
            m = re.match(r'^jobs/([\w-]+)$', name)
            if m:
                job = jobs.jobs.get(m.group(1))
                if not job:
                    return self._error(404, 'no such job')
                return self._json(dict(job.summary(), output=job.lines))
            return self._error(404, 'not found')

        def _start_job(self, body):
            args = body.get('args')
            if not isinstance(args, list) or not args or not all(isinstance(a, str) for a in args):
                raise ValueError('args must be a non-empty list of strings')
            if len(args) > 64 or any(len(a) > 2048 or '\0' in a for a in args):
                raise ValueError('arguments too long')
            cmd = args[0]
            if cmd not in ALLOWED:
                raise ValueError('command not allowed: %s' % cmd)
            subs = ALLOWED[cmd]
            if subs is not None and (len(args) < 2 or args[1] not in subs):
                raise ValueError('sub-command not allowed: %s %s' % (cmd, args[1] if len(args) > 1 else ''))
            stdin_text = None
            if cmd == 'login':
                stdin_text = str(body.get('key') or '').strip()
                if not re.match(r'^[A-Za-z0-9+/=._-]{10,512}$', stdin_text):
                    raise ValueError('that does not look like a Nexus API key')
                args = ['login']
            game = self._game(body.get('game'))
            label = str(body.get('label') or ' '.join(args))[:160]
            job = jobs.start(args, game, stdin_text, label)
            if job is None:
                return self._error(409, 'another action is still running')
            return self._json(job.summary(), 201)

        def _events(self, jid, start):
            job = jobs.jobs.get(jid)
            if not job:
                return self._error(404, 'no such job')
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Cache-Control', 'no-cache')
            self.send_header('Connection', 'close')
            self.end_headers()
            self.close_connection = True
            idx = max(0, start)
            try:
                while True:
                    with job.cond:
                        if idx >= len(job.lines) and job.status == 'running':
                            job.cond.wait(timeout=15)
                        new = job.lines[idx:]
                        status, code = job.status, job.code
                    if new:
                        payload = json.dumps({'from': idx, 'lines': new})
                        self.wfile.write(('event: lines\ndata: %s\n\n' % payload).encode())
                        idx += len(new)
                    elif status == 'running':
                        self.wfile.write(b': keep-alive\n\n')
                    if status != 'running' and idx >= len(job.lines):
                        self.wfile.write(('event: end\ndata: %s\n\n' % json.dumps({'status': status, 'code': code})).encode())
                        self.wfile.flush()
                        return
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                return

        def _static(self, path):
            rel = path.lstrip('/')
            target = (static_root / rel).resolve() if rel else static_root / 'index.html'
            if static_root not in target.parents and target != static_root / 'index.html':
                return self._error(404, 'not found')
            if rel.startswith('assets/') and not target.is_file():
                return self._error(404, 'not found')
            if rel == '' or rel == 'index.html' or not target.is_file():
                # single-page app: unknown paths get the app shell
                return self._send(200, index_html(), 'text/html; charset=utf-8',
                                  {'Cache-Control': 'no-store',
                                   'Content-Security-Policy': "default-src 'self'; img-src 'self' https: data:; "
                                                              "style-src 'self' 'unsafe-inline'; connect-src 'self'; "
                                                              "font-src 'self' data:; frame-ancestors 'none'"})
            ctype = mimetypes.guess_type(str(target))[0] or 'application/octet-stream'
            cache = 'public, max-age=31536000, immutable' if '/assets/' in '/' + rel else 'no-cache'
            return self._send(200, target.read_bytes(), ctype, {'Cache-Control': cache})

    return Handler


def free_port(host, preferred):
    for port in [preferred] + [0]:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            try:
                s.bind((host, port))
                return s.getsockname()[1]
            except OSError:
                continue
    raise SystemExit('No free port')


def main():
    ap = argparse.ArgumentParser(description='modman web UI')
    ap.add_argument('--modman', required=True)
    ap.add_argument('--static', required=True)
    ap.add_argument('--port', type=int, default=7427)
    ap.add_argument('--no-open', action='store_true')
    ap.add_argument('--verbose', action='store_true')
    cfg = ap.parse_args()
    host = '127.0.0.1'
    cfg.port = free_port(host, cfg.port)
    cfg.token = secrets.token_urlsafe(32)
    cfg.jobs = Jobs(cfg.modman)
    httpd = ThreadingHTTPServer((host, cfg.port), make_handler(cfg))
    httpd.daemon_threads = True
    url = 'http://127.0.0.1:%d/' % cfg.port
    print('modman UI running at %s  (Ctrl+C to stop)' % url, flush=True)
    if not cfg.no_open:
        threading.Timer(0.4, lambda: webbrowser.open(url)).start()
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print('\nStopping.')
    finally:
        httpd.server_close()


if __name__ == '__main__':
    main()
