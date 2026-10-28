# modman

Nexus Mods manager for Linux (Rust rewrite). Work in progress.

- Design: `linux_mod_manager_tech_stack.md`
- Porting plan: `feature_parity_checklist.md`
- Old bash tool and web UI: `legacy/` (reference only)

```sh
cargo build      # core + CLI
cargo run -p modman-cli -- --help
./dev.sh         # desktop app in Tauri dev mode (installs npm deps on first run)
```

`dev.sh` needs the webkit2gtk-4.1 dev packages and Node/npm on your `PATH`.
It is equivalent to `cd apps/desktop && npm install && npm run tauri dev`.

Licence: GPL-3.0-or-later.
