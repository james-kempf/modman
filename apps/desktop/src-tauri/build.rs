// SPDX-License-Identifier: GPL-3.0-or-later
include!("src/command_names.rs");

fn main() {
    // App commands get generated `allow-<command>` permissions, so the
    // capability file grants exactly these and nothing implicit.
    let attrs = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attrs).expect("tauri build script failed");
}
