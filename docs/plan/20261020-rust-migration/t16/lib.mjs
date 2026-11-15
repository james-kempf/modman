// T16 helpers. Run with NODE_PATH pointing at a node_modules holding playwright + @axe-core/playwright.
import { createRequire } from "node:module";
const require = createRequire(process.env.PW_MODULES + "/");
export const { chromium } = require("playwright");
export const AxeBuilder = require("@axe-core/playwright").default;
export const URL = process.env.T16_URL ?? "http://localhost:5199/";
export const OUT = new globalThis.URL("./screens/", import.meta.url).pathname;
export const VIEWS = ["overview", "mods", "install", "collections", "requirements", "activity", "settings"];

export async function open(browser, { gamepad = false } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, colorScheme: "dark" });
  const page = await ctx.newPage();
  const logs = { console: [], pageerr: [], net: [] };
  page.on("console", (m) => m.type() === "error" && logs.console.push(m.text()));
  page.on("pageerror", (e) => logs.pageerr.push(String(e)));
  page.on("requestfailed", (r) => logs.net.push(r.url() + " " + r.failure()?.errorText));
  page.on("response", (r) => r.status() >= 400 && logs.net.push(r.status() + " " + r.url()));
  if (gamepad) {
    await page.addInitScript(() => {
      const pad = {
        id: "T16 virtual pad (STANDARD GAMEPAD)", index: 0, connected: true, mapping: "standard", timestamp: 0,
        buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })),
        axes: [0, 0, 0, 0],
      };
      window.__pad = pad;
      navigator.getGamepads = () => [pad, null, null, null];
      window.__padConnect = () => { const e = new Event("gamepadconnected"); e.gamepad = pad; window.dispatchEvent(e); };
    });
  }
  await page.goto(URL);
  await page.waitForSelector("[data-tab][aria-current='page']");
  await page.waitForTimeout(400);
  return { ctx, page, logs };
}

export const active = (page) =>
  page.evaluate(() => {
    const e = document.activeElement;
    if (!e || e === document.body) return "BODY";
    const lab = e.getAttribute("aria-label") || e.getAttribute("data-tab") || (e.labels?.[0]?.textContent) || e.textContent.trim().slice(0, 40);
    return `${e.tagName.toLowerCase()}:${lab}`;
  });

/** Press a pad button for ~3 frames then release. */
export async function padPress(page, btn, holdMs = 80) {
  await page.evaluate((b) => { const x = window.__pad.buttons[b]; x.pressed = true; x.value = 1; window.__pad.timestamp++; }, btn);
  await page.waitForTimeout(holdMs);
  await page.evaluate((b) => { const x = window.__pad.buttons[b]; x.pressed = false; x.value = 0; window.__pad.timestamp++; }, btn);
  await page.waitForTimeout(80);
}
export const PAD = { A: 0, B: 1, LB: 4, RB: 5, UP: 12, DOWN: 13, LEFT: 14, RIGHT: 15 };

const KEY = { up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight" };
const PADDIR = { up: 12, down: 13, left: 14, right: 15 };

/** Send one directional input. */
export async function move(page, dir, mode) {
  if (mode === "pad") { await padPress(page, PADDIR[dir]); await page.waitForTimeout(120); }
  else { await page.keyboard.press(KEY[dir]); await page.waitForTimeout(30); }
}

/**
 * Find a directional path from the current focus to the element matching `css`
 * (discovery uses programmatic focus to explore edges), then reset focus and
 * replay the path with real key / pad input only. Returns the path; throws if
 * the target is unreachable or the replay does not land on it.
 */
export async function navTo(page, css, mode = "key", opts = {}) {
  let err;
  for (let i = 0; i < 3; i++) {
    try { return await navTo1(page, css, mode, opts); } catch (e) { err = e; if (!/replay/.test(e.message)) throw e; }
  }
  throw err;
}
async function navTo1(page, css, mode = "key", { maxNodes = 80 } = {}) {
  const ids = await page.evaluate((css) => {
    const sel = "button:not([disabled]),a[href],input:not([disabled]):not([type=hidden]),textarea:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex='-1'])";
    document.querySelectorAll("[data-nav-id]").forEach((e) => e.removeAttribute("data-nav-id"));
    const scopes = document.querySelectorAll("[data-nav-scope]");
    const scope = scopes.length ? scopes[scopes.length - 1] : document.body;
    const els = [...scope.querySelectorAll(sel)].filter((e) => !e.closest("[inert]"));
    els.forEach((e, i) => e.setAttribute("data-nav-id", i));
    const tgt = scope.querySelector(css);
    const cur = document.activeElement;
    return { start: cur?.getAttribute("data-nav-id") ?? null, target: tgt?.getAttribute("data-nav-id") ?? null };
  }, css);
  if (ids.target == null) throw new Error(`navTo: no focusable target ${css}`);
  if (ids.start == null) throw new Error(`navTo: focus not on a focusable (${await active(page)})`);
  if (ids.start === ids.target) return [];
  const focusId = (i) => page.evaluate((i) => document.querySelector(`[data-nav-id='${i}']`).focus(), i);
  const prev = { [ids.start]: null };
  const q = [ids.start];
  let found = false, n = 0;
  while (q.length && !found && n++ < maxNodes) {
    const cur = q.shift();
    for (const d of ["down", "right", "up", "left"]) {
      await focusId(cur);
      await move(page, d, mode);
      const t = await page.evaluate(() => document.activeElement?.getAttribute("data-nav-id") ?? null);
      if (t != null && !(t in prev)) { prev[t] = [cur, d]; q.push(t); if (t === ids.target) { found = true; break; } }
    }
  }
  if (!found) throw new Error(`navTo: ${css} unreachable by ${mode} arrows from ${ids.start}`);
  const path = []; for (let c = ids.target; prev[c]; c = prev[c][0]) path.unshift(prev[c][1]);
  await focusId(ids.start);
  for (const d of path) await move(page, d, mode);
  const ok = await page.evaluate((css) => document.activeElement?.matches(css), css);
  if (!ok) throw new Error(`navTo replay of [${path}] ended on ${await active(page)}, not ${css}`);
  return path;
}

/** Mark the first focusable whose aria-label / text / label equals `text` (in the top nav scope); returns its CSS. */
export async function goal(page, text) {
  const ok = await page.evaluate((text) => {
    document.querySelectorAll("[data-goal]").forEach((e) => e.removeAttribute("data-goal"));
    const scopes = document.querySelectorAll("[data-nav-scope]");
    const scope = scopes.length ? scopes[scopes.length - 1] : document.body;
    const names = (e) => [e.getAttribute("aria-label"), e.textContent.trim(), e.labels?.[0]?.textContent.trim(), e.id].filter(Boolean);
    const el = [...scope.querySelectorAll("button,input,pre,[tabindex]")].find((e) => !e.closest("[inert]") && names(e).some((n) => n === text || n.startsWith(text)));
    if (el) el.setAttribute("data-goal", "");
    return !!el;
  }, text);
  if (!ok) throw new Error(`goal: nothing named ${text}`);
  return "[data-goal]";
}
