// SPDX-License-Identifier: GPL-3.0-or-later
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, expect } from "vitest";
import { useUi } from "../app/store";
import { isToastTitle } from "../app/toastText";


// T15 copy rule, enforced across the whole suite: every toast shown in any test has a 1-2 word title with no
// digits, no colon and no game name (the specifics go in its detail).
const GAME_WORDS = /\b(Skyrim|Cyberpunk|Fallout|Stardew|Palworld|Oblivion)\b/i;
const titles: string[] = [];
useUi.subscribe((s, p) => {
  if (s.toast && s.toast !== p.toast) titles.push(s.toast.text);
});
// One hook: the DOM is always cleaned up (no vitest globals, so testing-library cannot register this itself),
// even when the title check fails, so one bad title cannot cascade into the next test.
afterEach(() => {
  const bad = titles.filter((t) => !isToastTitle(t) || GAME_WORDS.test(t));
  titles.length = 0;
  try {
    expect(bad, "toast titles must be 1-2 words, no digits, no ':', no game name").toEqual([]);
  } finally {
    cleanup();
  }
});
