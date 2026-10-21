# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds modman-desktop (Tauri v2) and the `modman` CLI. cargo and npm
# fetch dependencies, so build with network access (e.g. COPR with
# "enable internet" or `mock --enable-network`).
# Runtime deps (spikes.md §2, §6, §7): webkit2gtk4.1 (+ libsoup3, gtk3) and
# libarchive (.rar via compress-tools). libloot is pure Rust and SQLite is
# compiled in. No 7z/unrar binary is used at runtime.

%global debug_package %{nil}

Name:           modman
Version:        0.1.0
Release:        1%{?dist}
Summary:        Nexus Mods manager for Steam/Proton games (GUI + CLI)
License:        GPL-3.0-or-later
# Upstream URL is not set yet (Cargo.toml `repository = ""`); set it before publishing.
URL:            https://github.com/modman/modman
Source0:        %{url}/archive/refs/tags/v%{version}/%{name}-%{version}.tar.gz
ExclusiveArch:  x86_64

BuildRequires:  cargo >= 1.93
BuildRequires:  rust >= 1.93
BuildRequires:  gcc
BuildRequires:  nodejs >= 20
BuildRequires:  npm
BuildRequires:  pkgconfig(webkit2gtk-4.1)
BuildRequires:  pkgconfig(javascriptcoregtk-4.1)
BuildRequires:  pkgconfig(libsoup-3.0)
BuildRequires:  pkgconfig(gtk+-3.0)
BuildRequires:  pkgconfig(libarchive)
BuildRequires:  pkgconfig(dbus-1)
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

Requires:       webkit2gtk4.1
Requires:       libsoup3
Requires:       gtk3
Requires:       libarchive
Requires:       hicolor-icon-theme
Recommends:     libnotify
Recommends:     xdg-utils

%description
modman installs, orders and deploys Nexus Mods for Steam and Proton games on
Linux and the Steam Deck: FOMOD installers, collections, LOOT plugin sorting
and nxm:// links. Ships the desktop app (modman-desktop) and the modman CLI,
which is the nxm:// handler.

%prep
%autosetup -n %{name}-%{version}

%build
pushd apps/desktop
npm ci
npm run build
popd
cargo build --release --locked -p modman-cli
# tauri/custom-protocol serves the embedded dist instead of the dev URL.
cargo build --release --locked -p modman-desktop --features tauri/custom-protocol

%install
install -Dm755 target/release/modman-desktop %{buildroot}%{_bindir}/modman-desktop
install -Dm755 target/release/modman %{buildroot}%{_bindir}/modman
install -Dm644 packaging/common/org.modman.app.desktop %{buildroot}%{_datadir}/applications/org.modman.app.desktop
install -Dm644 packaging/common/modman-nxm.desktop %{buildroot}%{_datadir}/applications/modman-nxm.desktop
install -Dm644 packaging/common/org.modman.app.metainfo.xml %{buildroot}%{_metainfodir}/org.modman.app.metainfo.xml
install -Dm644 apps/desktop/src-tauri/icons/32x32.png %{buildroot}%{_datadir}/icons/hicolor/32x32/apps/org.modman.app.png
install -Dm644 apps/desktop/src-tauri/icons/128x128.png %{buildroot}%{_datadir}/icons/hicolor/128x128/apps/org.modman.app.png
install -Dm644 apps/desktop/src-tauri/icons/128x128@2x.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/org.modman.app.png

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/org.modman.app.desktop
desktop-file-validate %{buildroot}%{_datadir}/applications/modman-nxm.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/org.modman.app.metainfo.xml
cargo test --release --locked -p modman-core -p modman-cli

%files
%license LICENSE
%doc README.md packaging/README.md
%{_bindir}/modman
%{_bindir}/modman-desktop
%{_datadir}/applications/org.modman.app.desktop
%{_datadir}/applications/modman-nxm.desktop
%{_metainfodir}/org.modman.app.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/org.modman.app.png

%changelog
* Wed Oct 21 2026 James Kempf <jamesk4321@gmail.com> - 0.1.0-1
- Initial package (Rust rewrite)
