// SPDX-License-Identifier: GPL-3.0-or-later
include!("src/command_names.rs");

fn main() {
    // tauri-codegen embeds the window icon from an OUT_DIR cache without
    // tracking the source files, so icon changes would never rebuild.
    println!("cargo:rerun-if-changed=icons");
    // App commands get generated `allow-<command>` permissions, so the
    // capability file grants exactly these and nothing implicit.
    let attrs = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attrs).expect("tauri build script failed");
}
