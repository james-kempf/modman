# modman

Nexus Mods manager for Linux (Rust rewrite). Work in progress.

- Design: `linux_mod_manager_tech_stack.md`
- Porting plan: `feature_parity_checklist.md`
- Old bash tool and web UI: `legacy/` (reference only)

```sh
cargo build      # core + CLI
cargo run -p modman-cli -- --help
cd apps/desktop && npm install && npm run tauri dev   # needs webkit2gtk-4.1 dev packages
```

Licence: GPL-3.0-or-later.
