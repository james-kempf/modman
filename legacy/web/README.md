# modman web UI

Start it with `modman ui` (or `modman ui --port 8080 --no-open`). It serves the
prebuilt `dist/` through `server.py`, which only needs Python 3, so nothing here
has to be installed to use it (Steam Deck included). `modman ui desktop` adds a
launcher to the application menu.

The server listens on 127.0.0.1 only, rejects foreign Host headers and requires a
per-launch token embedded in the page. Every action runs the `modman` script.

## Developing

```sh
cd web
npm install
modman ui --no-open --port 7427 &   # API for the dev server
npm run dev                          # http://localhost:5173, proxies /api
npm run build                        # writes dist/ — commit it
```

Stack: React 19, Vite, Tailwind CSS 4, Framer Motion, Phosphor icons, Geist fonts
(bundled, no network fonts).
