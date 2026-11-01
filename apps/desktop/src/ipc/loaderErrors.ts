// SPDX-License-Identifier: GPL-3.0-or-later
// User-facing text and the suggested next step for each loader error code
// (`CmdError.code`, from crates/vessel-core/src/loaders/codes.rs). The code
// list is pinned by loaderErrorCodes.json, which the Rust test writes.

import type { CmdError } from "./types";

export type LoaderErrorAction =
  | "retry"
  | "pick_runtime"
  | "close_game"
  | "verify_files"
  | "adopt"
  | "open_log"
  | "open_settings"
  | "none";

export interface LoaderErrorInfo {
  title: string;
  explanation: string;
  action: LoaderErrorAction;
}

const m = (title: string, explanation: string, action: LoaderErrorAction): LoaderErrorInfo => ({
  title,
  explanation,
  action,
});

export const LOADER_ERRORS: Readonly<Record<string, LoaderErrorInfo>> = {
  loader_conflict: m(
    "Files already in use",
    "Some files this loader needs are already managed by a mod or another loader. Remove or disable that one first.",
    "none",
  ),
  loader_archive_required: m(
    "Loader download needed",
    "This step needs the loader's downloaded archive. Download it again, then retry.",
    "retry",
  ),
  loader_payload: m(
    "Loader definition problem",
    "The loader's setup for this game is invalid. Check the log for details.",
    "open_log",
  ),
  loader_unsafe_path: m(
    "Unsafe file path",
    "The loader tried to use a file outside the game folder, so Vessel stopped it.",
    "open_log",
  ),
  loader_unsafe_for_cmd: m(
    "Unsafe installer argument",
    "An installer argument contains characters Windows would misread, so Vessel did not run it.",
    "open_log",
  ),
  loader_manual_install_detected: m(
    "Loader installed outside Vessel",
    "This loader is already in the game folder but Vessel did not install it. Adopt it to let Vessel manage it.",
    "adopt",
  ),
  loader_ambiguous_match: m(
    "Unclear loader archive",
    "This download looks like more than one loader, so Vessel cannot tell which to install.",
    "none",
  ),
  loader_duplicate_key: m(
    "Loader listed twice",
    "The same loader is defined twice for this game. Check the game's configuration.",
    "open_log",
  ),
  loader_unknown_loader: m(
    "Unknown loader",
    "Vessel does not know this loader for the current game.",
    "none",
  ),
  loader_no_compat_tool: m(
    "No Proton version chosen",
    "Steam has no Proton version set for this game. Pick one in Steam or in Vessel's settings.",
    "open_settings",
  ),
  loader_compat_tool_missing: m(
    "Proton not found",
    "The Proton version this game uses is not installed in any Steam library. Install it from Steam.",
    "open_settings",
  ),
  loader_proton_untrusted: m(
    "Proton not trusted",
    "Vessel refused to use this Proton install because it did not pass safety checks.",
    "open_log",
  ),
  loader_slr_missing: m(
    "Steam Linux Runtime missing",
    "Proton needs the Steam Linux Runtime. Install it from Steam, then retry.",
    "retry",
  ),
  loader_flatpak_unsupported: m(
    "Not available in the Flatpak",
    "Windows installers cannot run through Proton while Vessel runs as a Flatpak.",
    "none",
  ),
  loader_flatpak_steam_unsupported: m(
    "Flatpak Steam not supported",
    "Windows installers cannot run through the Proton of Steam installed as a Flatpak.",
    "none",
  ),
  loader_game_running: m(
    "Game is running",
    "Close the game, then try again.",
    "close_game",
  ),
  loader_not_fully_installed: m(
    "Game needs an update",
    "Steam says the game is not fully installed or has an update pending. Update it in Steam, then retry.",
    "retry",
  ),
  loader_prefix_missing: m(
    "Game not started yet",
    "The game's Proton prefix does not exist. Start the game once from Steam, then retry.",
    "retry",
  ),
  loader_unknown_runtime: m(
    "Native or Proton?",
    "Vessel cannot tell whether this game runs natively or through Proton. Choose which one it uses.",
    "pick_runtime",
  ),
  loader_host_mismatch: m(
    "Installer not supported here",
    "This kind of installer cannot run on this system.",
    "none",
  ),
  loader_spawn: m(
    "Installer did not start",
    "Vessel could not start the installer. Check the log for details.",
    "open_log",
  ),
  loader_layout_mismatch: m(
    "Not a valid loader archive",
    "This download does not look like the loader it claims to be. Download it again from the official page.",
    "none",
  ),
  loader_verify_failed: m(
    "Loader step did not finish",
    "The installer ran, but the game folder does not look right afterwards. Check the log for details.",
    "open_log",
  ),
  loader_verify_failed_steam: m(
    "Game files changed",
    "Some game files are not the originals. Verify the game files in Steam, then try again.",
    "verify_files",
  ),
  loader_consent: m(
    "Plan changed",
    "Something changed since you reviewed this step. Review it again before continuing.",
    "retry",
  ),
  loader_already_installed: m(
    "Already installed",
    "This loader is already installed. Upgrade or repair it instead.",
    "none",
  ),
  loader_not_installed: m(
    "Not installed",
    "This loader is not installed for the current game.",
    "none",
  ),
  loader_store: m(
    "Database error",
    "Vessel could not read or write its database. Check the log for details.",
    "open_log",
  ),
  loader_io: m(
    "File error",
    "Vessel could not read or write a file. Check the log for details.",
    "open_log",
  ),
  launch_conflict: m(
    "Launch options conflict",
    "The game's current Steam launch options clash with what the loader needs. Edit them in Steam.",
    "none",
  ),
  launch_invalid: m(
    "Invalid launch options",
    "The game's Steam launch options could not be understood.",
    "none",
  ),
  launch_steam_running: m(
    "Steam is running",
    "Close Steam completely, then retry. Steam overwrites launch options when it exits.",
    "retry",
  ),
  launch_changed: m(
    "Steam settings changed",
    "Steam's settings file changed while Vessel was editing it. Nothing was written; try again.",
    "retry",
  ),
  launch_no_account: m(
    "No Steam account",
    "No Steam account was found. Start Steam and sign in once, then retry.",
    "retry",
  ),
  launch_unknown_account: m(
    "Unknown Steam account",
    "The chosen Steam account was not found. Pick another in settings.",
    "open_settings",
  ),
  launch_nothing_to_apply: m(
    "No launch options needed",
    "This game has no launch options to set.",
    "none",
  ),
  launch_unknown_game: m(
    "Unknown game",
    "Vessel does not know this game.",
    "none",
  ),
  launch_vdf: m(
    "Cannot read Steam settings",
    "Steam's settings file could not be read. Check the log for details.",
    "open_log",
  ),
  launch_store: m(
    "Database error",
    "Vessel could not read or write its database. Check the log for details.",
    "open_log",
  ),
  launch_io: m(
    "File error",
    "Vessel could not read or write a Steam file. Check the log for details.",
    "open_log",
  ),
};

/** The mapping for a rejection value, or `undefined` when it has no known code. */
export function loaderErrorInfo(e: unknown): LoaderErrorInfo | undefined {
  if (!e || typeof e !== "object" || !("code" in e)) return undefined;
  const code = (e as CmdError).code;
  return code !== undefined && Object.hasOwn(LOADER_ERRORS, code) ? LOADER_ERRORS[code] : undefined;
}
