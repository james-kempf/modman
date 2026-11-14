// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";

import type { ModRestoreResult, UninstallResult } from "./types";

// Fails when the TS contract stops accepting the legacy UninstallResult
// (no `restore` key, as the CLI and old backends send it) or the new one.
describe("UninstallResult shapes", () => {
  const legacy = `{"done":true,"mods":1,"archives":[],"after":{"deployed":null,"needs_deploy":false}}`;
  const withRestore = `{"done":true,"mods":2,"archives":[],"after":{"deployed":null,"needs_deploy":true},
    "restore":[{"token":"0f3a","uid":"A","name":"Alpha","restorable":true},
               {"token":"9bc1","uid":"F","name":"Folder","restorable":false,"reason":"folder_install"}]}`;
  const undoable = (r: UninstallResult) =>
    (r.restore ?? []).filter((e) => e.restorable).map((e) => e.token);

  it("parses the legacy shape with no restore entries", () => {
    const r = JSON.parse(legacy) as UninstallResult;
    expect(r.restore).toBeUndefined();
    expect(undoable(r)).toEqual([]);
  });

  it("parses restore entries; only restorable ones are undoable", () => {
    const r = JSON.parse(withRestore) as UninstallResult;
    expect(undoable(r)).toEqual(["0f3a"]);
    expect(r.restore?.[1].reason).toBe("folder_install");
    expect(r.restore?.[0].reason).toBeUndefined();
  });

  it("parses a mod_restore result with typed errors", () => {
    const r = JSON.parse(`{"results":[
      {"token":"x","uid":null,"name":null,"ok":false,"error":{"kind":"expired","message":"m"},"warnings":[]},
      {"token":"y","uid":"A","name":"Alpha","ok":true,"warnings":["w"]}],
      "restored":1,"after":{"deployed":null,"needs_deploy":true}}`) as ModRestoreResult;
    expect(r.results.map((x) => x.error?.kind ?? "ok")).toEqual(["expired", "ok"]);
  });
});
