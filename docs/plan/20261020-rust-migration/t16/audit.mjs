// Per-view keyboard audit: arrow-key focus graph (reachability + traps), PageUp/PageDown,
// Esc, focus ring, horizontal overflow, hover-only rules, axe-core, screenshots.
import { writeFileSync } from "node:fs";
import { chromium, AxeBuilder, OUT, VIEWS, open, active } from "./lib.mjs";

const browser = await chromium.launch();
const { page, logs } = await open(browser);
const result = { views: {}, global: {} };

// Start state: focus should be on the current tab (Overview).
result.global.initialFocus = await active(page);

// Hover-only CSS: :hover rules that change display/visibility/opacity, and group-hover.
result.global.hoverOnlyRules = await page.evaluate(() => {
  const out = [];
  for (const s of document.styleSheets) {
    let rules; try { rules = s.cssRules; } catch { continue; }
    const walk = (rs) => { for (const r of rs) {
      if (r.cssRules) walk(r.cssRules);
      if (r.selectorText && /:hover/.test(r.selectorText) && /(display|visibility|opacity|pointer-events)\s*:/.test(r.cssText)) out.push(r.cssText.slice(0, 200));
    } };
    walk(rules);
  }
  return out;
});

for (let vi = 0; vi < VIEWS.length; vi++) {
  const v = VIEWS[vi];
  const r = (result.views[v] = {});
  // Navigate keyboard-only: from tab rail, PageDown vi times from overview.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(100);
  r.tabAfterNav = await page.evaluate(() => document.querySelector("[data-tab][aria-current='page']")?.dataset.tab);
  r.focusAfterPageNav = await active(page);
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT}${vi + 1}-${v}.png` });

  // Overflow
  r.overflow = await page.evaluate(() => {
    const d = document.documentElement, m = document.querySelector("main");
    const wide = [...document.querySelectorAll("main *")].filter((e) => e.getBoundingClientRect().right > window.innerWidth + 1).map((e) => e.tagName + "." + e.className.toString().slice(0, 40)).slice(0, 5);
    return { docScrollW: d.scrollWidth, clientW: d.clientWidth, mainScrollW: m.scrollWidth, mainClientW: m.clientWidth, wide };
  });

  // Focus graph via arrows
  const g = await page.evaluate(() => {
    const sel = "button:not([disabled]),a[href],input:not([disabled]):not([type=hidden]),textarea:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex='-1'])";
    const els = [...document.querySelectorAll(sel)].filter((e) => !e.closest("[inert]"));
    els.forEach((e, i) => e.setAttribute("data-t16", i));
    return els.map((e) => (e.getAttribute("aria-label") || e.dataset.tab || e.labels?.[0]?.textContent || e.textContent.trim()).slice(0, 40) + "|" + e.tagName);
  });
  const edges = {};
  for (let i = 0; i < g.length; i++) {
    edges[i] = {};
    for (const k of ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]) {
      await page.evaluate((i) => document.querySelector(`[data-t16='${i}']`).focus(), i);
      await page.keyboard.press(k);
      edges[i][k] = await page.evaluate(() => document.activeElement?.getAttribute("data-t16") ?? "BODY");
    }
  }
  const start = String(await page.evaluate(() => document.querySelector("[data-tab][aria-current='page']").getAttribute("data-t16")));
  const reach = new Set([start]); const q = [start];
  while (q.length) { const n = q.shift(); for (const t of Object.values(edges[n] ?? {})) if (t !== "BODY" && !reach.has(t)) { reach.add(t); q.push(t); } }
  r.focusables = g.length;
  r.unreachable = g.map((n, i) => [i, n]).filter(([i]) => !reach.has(String(i))).map(([i, n]) => n);
  // Trap: node from which tab rail is not reachable via arrows (Esc also escapes, recorded separately)
  const tabIdx = new Set(g.map((n, i) => [n, i]).filter(([n]) => n.endsWith("|BUTTON")).map(([, i]) => String(i)).filter((i) => 1));
  const railIdx = await page.evaluate(() => [...document.querySelectorAll("[data-tab]")].map((e) => e.getAttribute("data-t16")));
  r.traps = [];
  for (const s of reach) {
    const seen = new Set([s]); const qq = [s]; let ok = false;
    while (qq.length && !ok) { const n = qq.shift(); if (railIdx.includes(n)) ok = true; for (const t of Object.values(edges[n] ?? {})) if (t !== "BODY" && !seen.has(t)) { seen.add(t); qq.push(t); } }
    if (!ok) r.traps.push(g[s]);
  }
  r.toBody = Object.entries(edges).flatMap(([i, e]) => Object.entries(e).filter(([, t]) => t === "BODY").map(([k]) => g[i] + " " + k));
  // Esc from deepest element returns to tab
  const last = [...reach].map(Number).sort((a, b) => b - a)[0];
  await page.evaluate((i) => document.querySelector(`[data-t16='${i}']`).focus(), last);
  await page.keyboard.press("Escape");
  r.escFromLast = { from: g[last], to: await active(page) };

  // Focus ring on a button in main
  r.focusRing = await page.evaluate(() => {
    const b = document.querySelector("main button:not([disabled])") || document.querySelector("[data-tab]");
    b.blur(); const s0 = getComputedStyle(b); const before = s0.outlineStyle + s0.outlineWidth + s0.boxShadow;
    b.focus(); const s1 = getComputedStyle(b);
    return { el: b.textContent.trim().slice(0, 30), before, after: s1.outlineStyle + " " + s1.outlineWidth + " " + s1.outlineColor + " / " + s1.boxShadow.slice(0, 60) };
  });
  await page.screenshot({ path: `${OUT}${vi + 1}-${v}-focus.png` });

  // axe
  const ax = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"]).analyze();
  r.axe = ax.violations.map((x) => ({ id: x.id, impact: x.impact, n: x.nodes.length, help: x.help, targets: x.nodes.slice(0, 3).map((n) => n.target.join(" ")) }));

  // Next view
  await page.evaluate(() => document.querySelector("[data-tab][aria-current='page']").focus());
  await page.keyboard.press("PageDown");
  await page.waitForTimeout(200);
}
// PageUp wraps back?
await page.keyboard.press("PageUp");
result.global.pageUpFromWrap = await page.evaluate(() => document.querySelector("[data-tab][aria-current='page']")?.dataset.tab);
result.logs = logs;
writeFileSync(new URL("./audit.json", import.meta.url), JSON.stringify(result, null, 1));
console.log(JSON.stringify(result, null, 1));
await browser.close();
