# Packaging modman

Every format ships two programs:

- `modman-desktop`: the Tauri v2 desktop app.
- `modman`: the command-line tool. It is also the only `nxm://` handler (`Exec=modman nxm %u`, spikes.md §5). The GUI never registers the scheme.

| Format | Recipe | Build |
| :--- | :--- | :--- |
| AppImage | `apps/desktop/src-tauri/tauri.conf.json` `bundle` + `packaging/appimage/` | `packaging/appimage/build-appimage.sh` |
| deb, rpm (Tauri bundler) | `tauri.conf.json` `bundle.linux.deb` / `bundle.linux.rpm` | `cargo build --release -p modman-cli && (cd apps/desktop && npx tauri build --bundles deb,rpm)` |
| Flatpak | `packaging/flatpak/org.modman.app.yml` | see the header of the manifest |
| Arch (AUR) | `packaging/aur/PKGBUILD` | `makepkg -si` |
| Fedora/openSUSE RPM (source build) | `packaging/rpm/modman.spec` | `rpmbuild -ba` (needs network for cargo/npm) |

Shared files are in `packaging/common/`: `org.modman.app.desktop` (GUI menu entry), `modman-nxm.desktop` (nxm handler, `NoDisplay=true`) and `org.modman.app.metainfo.xml`.

The app identifier `org.modman.app` must not change. The GUI owns the D-Bus name `org.modman.app.SingleInstance`, and `modman nxm` forwards links to it.

## Runtime dependencies

These follow `docs/plan/20261020-rust-migration/spikes.md`.

| Dependency | Why | AppImage | deb / rpm / AUR | Flatpak |
| :--- | :--- | :--- | :--- | :--- |
| webkit2gtk-4.1, javascriptcoregtk-4.1, libsoup-3.0, gtk3 | GUI webview | bundled by linuxdeploy | declared (`libwebkit2gtk-4.1-0`, `webkit2gtk4.1`, `webkit2gtk-4.1`) | in `org.gnome.Platform` 50 |
| libarchive (`libarchive.so.13`) | `.rar` through compress-tools | bundled (CLI and GUI have RUNPATH `$ORIGIN/../lib`) | declared (`libarchive13t64 \| libarchive13`, `libarchive`) | in `org.gnome.Platform` 49/50/51 and `org.freedesktop.Platform` 25.08/26.08 (checked locally) |
| libloot | LOOT sorting | pure-Rust crate, compiled in: nothing to ship | same | same |
| SQLite | store | compiled in (rusqlite `bundled`) | same | same |
| 7z / unrar binaries | none | not used at runtime: `.7z` is `sevenz-rust2`, `.zip` is `zip`, `.rar` is libarchive | same | same |
| Secret Service, `notify-send`, `xdg-mime`, `systemctl --user` | key storage, notifications, nxm registration, autoupdate timer | optional, from the host | optdepends/Recommends | portal/D-Bus permissions in the manifest; no `systemctl` in the sandbox |

SteamOS ships libarchive because pacman links against it. That has not been checked on a real Deck, and the AppImage bundles its own copy anyway.

## AppImage

`build-appimage.sh` does four things:

1. Builds the CLI.
2. Runs the Tauri bundler. `bundle.linux.appimage.files` copies `target/release/modman` to `usr/bin/modman`.
3. Renames the linuxdeploy `AppRun` to `AppRun.gui` and installs `packaging/appimage/AppRun`.
4. Repacks the AppImage with the appimage plugin that Tauri caches in `~/.cache/tauri`.

The `AppRun` launcher dispatches on the arguments:

| Invocation | Runs |
| :--- | :--- |
| `modman.AppImage` (no arguments), `modman.AppImage --gui` | the desktop app (via `AppRun.gui`, which runs the GTK hooks) |
| `modman.AppImage <anything>` (`nxm <url>`, `autoupdate run`, `install ...`, `--version`) | the bundled `modman` CLI with the arguments unchanged |

**`WEBKIT_DISABLE_DMABUF_RENDERER`:** the launcher sets it to `1` for the GUI unless it is already set. Without it, WebKitGTK's DMA-BUF renderer crashes or shows a blank window on some Wayland/GPU setups, including the development machine. To opt back in, export `WEBKIT_DISABLE_DMABUF_RENDERER=0`. The Flatpak sets the same variable with `--env`. The deb/rpm/AUR builds start `modman-desktop` directly, so on an affected system the user has to export the variable until the app sets it itself (an open item for the Rust side).

### AppImage and the CLI path (nxm handler, autoupdate)

- `<AppImage> nxm register` writes `Exec="/path/to/modman.AppImage" nxm %u`. It uses `$APPIMAGE`, never the temporary `/tmp/.mount_*` path. The launcher sends that call to the CLI.
- `<AppImage> autoupdate enable` (the CLI from the AppImage) writes the systemd user unit with `ExecStart=/path/to/modman.AppImage autoupdate run`. The launcher sends that to the CLI too.
- Enabling autoupdate **from the GUI** inside an AppImage currently schedules the CLI under the temporary `/tmp/.mount_*` mount, and modman prints a warning. Use `<AppImage> autoupdate enable` from a terminal instead.
- If you move or rename the AppImage, run `nxm register` and `autoupdate enable` again.

## Flatpak notes

- Permissions: `~/.local/share/Steam`, `~/.steam`, the Flatpak Steam data dir, `/run/media` (Deck SD cards), `/media`, `/mnt`, Secret Service and notifications.
- The nxm entry is installed as `org.modman.app.nxm.desktop`, because Flatpak exports only files prefixed with the app id. Flatpak rewrites its `Exec` to `flatpak run --command=modman org.modman.app nxm %u`.
- **Hardlink caveat.** Each `--filesystem` grant is a separate bind mount inside the sandbox. `link(2)` fails with `EXDEV` across mounts even when the underlying disk is the same. modman keeps staging inside each Steam library (`<library>/.modman/staging/<game>`), so staging and the game folder normally sit under the same grant and hardlinks work. They can fail when:
  - the game is reached through one grant (e.g. `~/.steam/steam/...`) and staging through another (e.g. `~/.local/share/Steam/...`). modman detects the `st_dev` mismatch or the `EXDEV` error and falls back to copy (or reflink where the filesystem supports it), which uses more disk space;
  - the library is on an SD card (`/run/media/...`) and staging is on internal storage. Staging per library prevents this; do not move staging to `$HOME`.

  On the Deck, prefer the AppImage if you want guaranteed hardlinks. A real SD-card library under Flatpak is unverified.
- Autoupdate (the systemd user timer) is not supported inside the Flatpak: the sandbox has no `systemctl`.

## Steam Deck

### Add modman as a non-Steam game (Desktop Mode)

1. Download `modman_<version>_amd64.AppImage` to e.g. `~/Applications/` and make it executable (Dolphin: Properties, Permissions, "Is executable"; or `chmod +x`).
2. Start it once in Desktop Mode. Then run `~/Applications/modman_<version>_amd64.AppImage nxm register` in Konsole, so browser "Mod manager download" buttons open modman.
3. In Steam (Desktop Mode): **Games, Add a Non-Steam Game to My Library, Browse**. Pick the AppImage (set the file type filter to "All files"), then **Add Selected Programs**.
4. Optionally, in the shortcut's **Properties**, rename it to `modman` and set artwork. Leave the launch options empty: arguments switch the AppImage to CLI mode.
5. Flatpak: add **modman** from the list (it appears as an installed application), or Browse to `/var/lib/flatpak/exports/bin/org.modman.app`.

The window opens at 1280x800, the Deck's native resolution.

### Game Mode: getting nxm links in (paste flow)

Game Mode has no system browser, so clicking a Nexus "Mod manager download" button cannot reach a handler. Instead:

1. Open the mod's **Files** tab on nexusmods.com in any browser (Desktop Mode, the Steam overlay browser, or another device). Long-press or right-click **Mod manager download** and copy the link (`nxm://...`).
2. Open modman, go to the downloads/paste-link box and paste with **Ctrl+V**. Open the on-screen keyboard with **Steam + X**.
3. modman parses and queues the link exactly like a browser click. From a terminal the equivalent is `modman nxm 'nxm://...'`.

Links expire (the `expires` parameter), so paste them soon after copying.

### Controller: keyboard-mapped Steam Input layout (fallback)

The UI is fully keyboard-navigable. If gamepad input does not reach the webview in Game Mode, use a keyboard-mapped layout. In the shortcut's **Controller settings**, choose the template **Keyboard (WASD) and Mouse** and edit it as follows:

| Control | Mapping |
| :--- | :--- |
| D-pad up/down/left/right | Up / Down / Left / Right arrow |
| A | Enter |
| B | Escape |
| X | Space (toggle) |
| Y | Tab |
| L1 / R1 | Shift+Tab / Tab |
| Left stick | arrow keys (directional pad mode) |
| Right trackpad | mouse |
| R2 | left mouse click |
| Start | Ctrl+V (paste an nxm link) |

Save it as a personal layout named "modman keyboard" so it can be reused after an update.

## Validation

| Check | Command |
| :--- | :--- |
| desktop files | `desktop-file-validate packaging/common/*.desktop` |
| metainfo | `appstreamcli validate --no-net packaging/common/org.modman.app.metainfo.xml` |
| RPM spec | `rpmlint packaging/rpm/modman.spec` |
| PKGBUILD | `namcap packaging/aur/PKGBUILD` (on Arch) |
| Flatpak | `flatpak-builder --show-manifest packaging/flatpak/org.modman.app.yml` |
| licences/bans | `cargo deny --all-features check licenses bans sources` |
| CI workflow | `actionlint .github/workflows/ci.yml` |
