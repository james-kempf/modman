# Vessel

Nexus Mods manager for Linux (Rust rewrite). Work in progress.

- Design: `linux_mod_manager_tech_stack.md`
- Porting plan: `feature_parity_checklist.md`
- Packaging and Steam Deck setup: `packaging/README.md`
- Old bash tool and web UI: `legacy/` (reference only)

```sh
cargo build      # core + CLI
cargo run -p vessel-cli -- --help
./dev.sh         # desktop app in Tauri dev mode (installs npm deps on first run)
```

`dev.sh` needs the webkit2gtk-4.1 dev packages and Node/npm on your `PATH`.
It is equivalent to `cd apps/desktop && npm install && npm run tauri dev`.

## Migrating from modman

Vessel used to be called modman. The first time Vessel starts (the desktop app, or any `vessel` command other than help and `version`), it moves modman's data to the Vessel locations. You do not need to do anything. Package managers handle the package rename; see `packaging/README.md`, "Upgrading from modman".

### What is copied and what is moved

| Old | New | How |
| :--- | :--- | :--- |
| `~/.config/modman/` | `~/.config/vessel/` | copied, then verified by hash |
| `~/.local/share/modman/modman.db` (and `-wal`) | `~/.local/share/vessel/vessel.db` | copied, then verified by hash |
| `~/.local/state/modman/` (`modman.log`) | `~/.local/state/vessel/` (`vessel.log`) | copied, then verified by hash |
| everything else in `~/.local/share/modman/` (downloads, backups, per-game data) | `~/.local/share/vessel/` | moved, entry by entry |
| `<library>/.modman/staging/` in each Steam library | `<library>/.vessel/staging/` | moved |

- A move is a rename. If the target is on another filesystem, Vessel copies to `<target>.migrating`, checks file count, sizes and SHA-256 of every file, renames the copy into place and only then deletes the original.
- If staging is copied across filesystems, hardlink deployments still work, but they no longer share disk space with staging.

### Order of steps

1. Take the migration lock (`~/.local/share/vessel-migration.lock`) and check that modman is not running.
2. Stop and disable the old `modman-autoupdate.timer` (whether it was enabled is recorded first), and stop a `modman-autoupdate.service` run already in progress, before the database is copied.
3. Fold the old database's write-ahead log into `modman.db`.
4. Copy config, database and log.
5. Move bulk data.
6. Move each library's staging dir.
7. Rewrite the absolute paths stored in the database (mods, downloads, backups, deployments, collections, deploy journal), in one transaction.
8. Retarget symlink deployments at the new staging paths.
9. Carry over the integrations: keyring, systemd timer, nxm handler, Steam shortcut.
10. Remove the old directories.

Steps 3 to 8 must succeed, or the migration stops with an error. Old data is never deleted before its copy has been verified. Once bulk data has moved, finish the migration by starting Vessel again; do not go back to modman. If the old timer or service cannot be stopped (step 2), Vessel warns and continues. The integration steps never stop the migration (see Integrations).

### modman must be closed

If modman's window is open (it owns the D-Bus name `org.modman.app.SingleInstance`) or a modman deploy or purge holds `~/.local/share/modman/deploy.lock`, Vessel changes nothing and exits with `Close modman before starting Vessel`.

- The desktop app shows an error dialog titled `Vessel` (GTK, else `zenity`, `kdialog` or `notify-send`) with the message, for example `Close modman before starting Vessel (...)`, prints it to stderr and exits. No window opens and no Vessel dirs are created.
- `vessel` commands print it and exit non-zero. This includes `vessel nxm`, so browser download links are refused until modman is closed.

### Resuming

Progress is recorded per step in `~/.config/vessel/migration.json`. If the migration is interrupted (crash, power loss, error), the next start resumes at the first unfinished step. Leftover `*.migrating` copies are deleted and redone.

### When Vessel data already exists

If `~/.config/vessel`, `~/.local/share/vessel` or `~/.local/state/vessel` exists but `migration.json` does not, Vessel moves nothing. It reports the conflict and leaves the modman data untouched. Move one side away and restart to migrate.

Smaller conflicts during a migration keep both copies and are reported:

- An entry that exists in both data dirs with different content.
- A library that already has `.vessel/staging` next to `.modman/staging`.

### Offline libraries

A Steam library that is not mounted (for example an SD card that is not inserted) is marked offline. Vessel retries its staging move, path rewrite and symlink retargeting on every later start until the library is migrated. Libraries in conflict are retried the same way.

### Symlink deployments

Mods deployed as symlinks point at the old staging path. Vessel replaces each link atomically with one that points at the new staging path. A link that no longer points at the old staged file (you changed it) is skipped and reported. Hardlink, reflink and copy deployments need no change.

### The data-dir setting

The `data-dir` setting is rewritten only when it points inside the old modman data dir. A custom `data-dir` elsewhere is kept as is, and nothing under it is moved.

### Integrations

- **Nexus API key.** Moved from the keyring service `modman` to `vessel`, read back, then deleted from the old entry. An existing Vessel key is never overwritten. If the keyring is unreachable (no Secret Service, or no answer within 3 s), the migration completes with a warning, and Vessel moves the key the next time it reads it, retrying on each lookup until the keyring answers.
- **Auto-update timer.** The `vessel-autoupdate` units are installed with the same interval, and the timer is enabled only if the modman timer was. The old units are removed after the new ones are in place.
- **nxm:// handler.** A user-level `modman-nxm.desktop` is replaced with `vessel-nxm.desktop`. The default handler for `nxm://` changes only where it pointed at `modman-nxm.desktop`. A handler you chose yourself is left alone.
- **Steam shortcut.** A non-Steam shortcut named `modman` is renamed to `Vessel` and pointed at the Vessel desktop app, in every Steam account. Its appid is kept, so playtime, controller layout and artwork survive. The original is saved as `shortcuts.vdf.vessel-bak`. If Steam is running, restart it to see the change.

If a program is missing, the step finishes without it and is not retried:

- `vessel` CLI not found: the old auto-update units are removed. If the old timer was enabled, Vessel warns that auto-update was turned off; run `vessel autoupdate enable`. The legacy nxm handler is left in place; run `vessel nxm register`.
- `vessel-desktop` not found: the legacy Steam shortcut is left unchanged. Use **Add to Steam** in Vessel's settings; it updates the entry in place and keeps its appid, playtime and artwork.

Transient failures (for example, the systemd user manager is unreachable) are reported and retried on the next start, and the old dirs are kept until they succeed.

### Cleanup

After every step has succeeded:

- The old config and state dirs are removed, but only if their files still match the hashes taken when they were copied. Otherwise they are kept and reported.
- The old database files and `deploy.lock` are removed from `~/.local/share/modman` if the database is unchanged since the copy. The dir itself is removed only if it is then empty; otherwise it is kept and reported.

Until then, the old dirs stay in place.

### Environment variables

Vessel ignores every `MODMAN_*` variable and prints one warning for each that is set:

| Variable | Warning |
| :--- | :--- |
| `MODMAN_X` for X in `DEBUG`, `NO_COLOR`, `LOOT_URL`, `NEXUS_URL`, `SYSTEMCTL`, `SANDBOX_ROOT`, `ART_PROBE_APPID`, `ART_PROBE_KIND`, `ART_PROBE_ROOT` | `MODMAN_X is deprecated and ignored; use VESSEL_X instead` |
| `MODMAN_DATA_DIR`, `MODMAN_CONFIG_DIR` | `MODMAN_DATA_DIR is used only by the legacy bash tool; ignored by Vessel` (same for `MODMAN_CONFIG_DIR`) |
| any other `MODMAN_X` | `MODMAN_X is not used by Vessel and is ignored` |

The migration sources are always the `modman` dirs next to Vessel's XDG dirs (`~/.config/modman`, `~/.local/share/modman`, `~/.local/state/modman` by default), never an environment variable.

### Flatpak

Inside the Flatpak, the old data reaches Vessel only through Flatpak's end-of-life rebase from `org.modman.app` to `org.vessel.app`. If you accept the rebase on `flatpak update`, Flatpak renames `~/.var/app/org.modman.app` to `~/.var/app/org.vessel.app`, and Vessel then migrates it as described above. Without the rebase (a fresh install, or `~/.var/app/org.vessel.app` already exists), the `org.vessel.app` sandbox has no access to `~/.var/app/org.modman.app`, and that data is not migrated. In that case, before the first start, copy `~/.var/app/org.modman.app/{config,data,.local/state}/modman` by hand into `~/.var/app/org.vessel.app/{config,data,.local/state}/modman`. Details: `packaging/README.md`, "Flatpak: what happens to `~/.var/app/org.modman.app`". Auto-update is not available inside the Flatpak.

## Licence

GPL-3.0-or-later.
