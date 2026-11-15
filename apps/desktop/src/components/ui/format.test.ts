// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { purgedMsg } from "../../views/Overview";
import { deployedMsg } from "../../app/run";
import { count } from "./format";

describe("count", () => {
  // Fails when toasts go back to "1 backup(s)" style or get the number wrong for one item.
  it("matches the noun to the number", () => {
    expect(count(1, "backup")).toBe("1 backup");
    expect(count(0, "backup")).toBe("0 backups");
    expect(count(3, "game file")).toBe("3 game files");
    expect(count(2, "mod", "mods")).toBe("2 mods");
  });
});

describe("deploy and purge toasts", () => {
  const plugins = { state: "failed" as const, detail: "read-only" };
  // Fails when the plugins.txt failure reads as a bare "plugins.txt failed" again, the purge no-op loses its
  // reason, or counts move back into the title.
  it("title + detail: what happened, then the counts and what failed", () => {
    const d = deployedMsg({ placed: 10, mods: 2, conflicts: [], skipped: [], plugins } as unknown as Parameters<typeof deployedMsg>[0]);
    expect(d).toEqual({ title: "Deployed", detail: "10 files · 2 mods · couldn't write plugins.txt: read-only" });
    expect(purgedMsg({ nothing_deployed: true } as unknown as Parameters<typeof purgedMsg>[0])).toEqual({
      title: "Nothing deployed",
      detail: "Nothing to purge: no mods are deployed",
    });
  });
});
