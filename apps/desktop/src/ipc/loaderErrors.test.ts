// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import codes from "./loaderErrorCodes.json";
import { LOADER_ERRORS, loaderErrorInfo, type LoaderErrorAction } from "./loaderErrors";

const ACTIONS: LoaderErrorAction[] = [
  "retry",
  "pick_runtime",
  "close_game",
  "verify_files",
  "adopt",
  "open_log",
  "open_settings",
  "none",
];

describe("loader error mapping", () => {
  // Catches: a code the Rust side emits with no UI text or action.
  it("maps every code from the Rust fixture", () => {
    for (const code of codes) {
      const info = LOADER_ERRORS[code];
      expect(info, code).toBeDefined();
      expect(info.title.trim(), code).not.toBe("");
      expect(info.explanation.trim(), code).not.toBe("");
      expect(ACTIONS, code).toContain(info.action);
    }
  });

  // Catches: stale entries for codes Rust no longer emits (typos included).
  it("has no codes beyond the fixture", () => {
    expect(Object.keys(LOADER_ERRORS).sort()).toEqual([...codes].sort());
  });

  // Catches: the actions the plan requires drifting.
  it("routes the key cases to their actions", () => {
    expect(LOADER_ERRORS.loader_verify_failed_steam.action).toBe("verify_files");
    expect(LOADER_ERRORS.loader_manual_install_detected.action).toBe("adopt");
    expect(LOADER_ERRORS.loader_game_running.action).toBe("close_game");
    expect(LOADER_ERRORS.loader_unknown_runtime.action).toBe("pick_runtime");
    expect(LOADER_ERRORS.launch_steam_running.action).toBe("retry");
    expect(LOADER_ERRORS.launch_steam_running.explanation).toMatch(/close Steam/i);
  });

  // Catches: lookups throwing on plain errors or matching prototype keys.
  it("looks up rejection values safely", () => {
    expect(loaderErrorInfo({ message: "x", code: "loader_game_running" })?.action).toBe("close_game");
    expect(loaderErrorInfo({ message: "x" })).toBeUndefined();
    expect(loaderErrorInfo({ message: "x", code: "toString" })).toBeUndefined();
    expect(loaderErrorInfo("boom")).toBeUndefined();
    expect(loaderErrorInfo(null)).toBeUndefined();
  });
});
