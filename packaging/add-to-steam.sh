#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Adds Vessel to Steam as a non-Steam game: asks how Vessel is installed,
# finds it on disk and runs its `vessel steam add`.
#
# Usage: packaging/add-to-steam.sh [appimage|flatpak|package] [AppImage path]
# With no arguments it asks. Steam picks the shortcut up on its next start.
set -euo pipefail

FLATPAK_ID=org.vessel.app
# Where AppImages usually live; searched two levels deep.
APPIMAGE_DIRS=(
    "$HOME/Applications"
    "$HOME/Documents/Applications"
    "$HOME/Downloads"
    "$HOME/Desktop"
    "$HOME/.local/bin"
    "$HOME/bin"
    /opt
)

die() { echo "error: $*" >&2; exit 1; }

# Prints the chosen line of stdin on stdout; the menu goes to stderr.
pick() {
    local prompt=$1 items=() i reply
    mapfile -t items
    ((${#items[@]})) || return 1
    if ((${#items[@]} == 1)); then
        printf '%s\n' "${items[0]}"
        return
    fi
    for i in "${!items[@]}"; do
        printf '  %d) %s\n' $((i + 1)) "${items[i]}" >&2
    done
    while :; do
        read -rp "$prompt [1-${#items[@]}]: " reply </dev/tty || return 1
        if [[ $reply =~ ^[0-9]+$ ]] && ((reply >= 1 && reply <= ${#items[@]})); then
            printf '%s\n' "${items[reply - 1]}"
            return
        fi
    done
}

add_appimage() {
    local image=${1:-}
    if [[ -z $image ]]; then
        local dirs=() d
        for d in "${APPIMAGE_DIRS[@]}"; do [[ -d $d ]] && dirs+=("$d"); done
        image=$( ((${#dirs[@]})) && find "${dirs[@]}" -maxdepth 2 -type f -iname 'vessel*.appimage' 2>/dev/null \
            | sort | pick "Which AppImage" ) || true
    fi
    if [[ -z $image ]]; then
        read -rep "No Vessel AppImage found. Path to it: " image </dev/tty
        image=${image/#\~/$HOME}
    fi
    [[ -f $image ]] || die "not a file: $image"
    image=$(readlink -f "$image")
    [[ -x $image ]] || die "$image is not executable; run: chmod +x '$image'"
    echo "Using $image"
    # With arguments the AppImage runs its bundled CLI; the shortcut then
    # points at the AppImage itself.
    "$image" steam add
}

add_flatpak() {
    command -v flatpak >/dev/null || die "flatpak is not installed"
    # The exported launcher is the host path Steam can start; the sandbox's
    # own /app/bin path does not exist outside it.
    local scope launcher
    for scope in user system; do
        flatpak info "--$scope" "$FLATPAK_ID" >/dev/null 2>&1 || continue
        if [[ $scope == user ]]; then
            launcher="${XDG_DATA_HOME:-$HOME/.local/share}/flatpak/exports/bin/$FLATPAK_ID"
        else
            launcher="/var/lib/flatpak/exports/bin/$FLATPAK_ID"
        fi
        echo "Using the $scope Flatpak ($launcher)"
        flatpak run "--$scope" --command=vessel "$FLATPAK_ID" steam add --exe "$launcher"
        return
    done
    die "the $FLATPAK_ID Flatpak is not installed"
}

add_package() {
    local cli
    cli=$(command -v vessel) || die "vessel is not on PATH; install the deb, rpm or AUR package first"
    command -v vessel-desktop >/dev/null || die "vessel-desktop is not on PATH; install the desktop app first"
    echo "Using $cli"
    # The CLI finds vessel-desktop next to it or on PATH.
    "$cli" steam add
}

kind=${1:-}
if [[ -z $kind ]]; then
    echo "How is Vessel installed?" >&2
    kind=$(printf '%s\n' \
        "appimage  (an AppImage file)" \
        "flatpak   (Flatpak / Discover)" \
        "package   (deb, rpm or AUR package)" \
        | pick "Installation type") || die "no choice made"
    kind=${kind%% *}
fi

case $kind in
    appimage) add_appimage "${2:-}" ;;
    flatpak) add_flatpak ;;
    package) add_package ;;
    *) die "unknown installation type '$kind' (appimage, flatpak or package)" ;;
esac
