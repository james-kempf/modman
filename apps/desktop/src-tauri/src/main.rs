// SPDX-License-Identifier: GPL-3.0-or-later
//! modman desktop app entry point.

use std::ffi::OsStr;

const DMABUF_VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// The value to set for [`DMABUF_VAR`]: `1` when unset or empty (as the
/// AppImage's AppRun does with `:=`), else none so an explicit `0` keeps the
/// DMA-BUF renderer. It crashes or shows a blank window on several
/// Wayland/NVIDIA/Mesa setups; this covers the deb/rpm/AUR builds.
fn dmabuf_default(current: Option<&OsStr>) -> Option<&'static str> {
    current.is_none_or(OsStr::is_empty).then_some("1")
}

fn main() {
    if let Some(v) = dmabuf_default(std::env::var_os(DMABUF_VAR).as_deref()) {
        // Still single-threaded: no other thread can read the environment.
        std::env::set_var(DMABUF_VAR, v);
    }
    modman_desktop::run();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches: a blank WebKit window on deb/rpm/AUR (no AppRun) and
    /// overriding a user's explicit opt-in.
    #[test]
    fn dmabuf_renderer_is_disabled_only_when_unset() {
        assert_eq!(dmabuf_default(None), Some("1"));
        assert_eq!(dmabuf_default(Some(OsStr::new(""))), Some("1"));
        assert_eq!(dmabuf_default(Some(OsStr::new("0"))), None);
        assert_eq!(dmabuf_default(Some(OsStr::new("1"))), None);
    }
}
