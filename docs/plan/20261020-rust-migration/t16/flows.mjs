// T16 end-to-end flows against the demo backend: keyboard-only and emulated gamepad.
// Every action is a real key press or pad button; programmatic focus is used only
// inside navTo's path discovery, which is then replayed with real input.
import { writeFileSync } from "node:fs";
import { chromium, OUT, open, active, navTo, goal, padPress, PAD } from "./lib.mjs";

const results = [];
const browser = await chromium.launch();
async function step(name, page, fn) {
  try { const info = await fn(); results.push({ name, ok: true, info }); }
  catch (e) {
    results.push({ name, ok: false, error: String(e.message ?? e).slice(0, 400), focus: await active(page).catch(() => "?") });
    await page.screenshot({ path: `${OUT}FAIL-${name.replace(/\W+/g, "_")}.png` }).catch(() => {});
  }
}
const expect = (c, msg) => { if (!c) throw new Error(msg); };
const curView = (p) => p.evaluate(() => document.querySelector("[data-tab][aria-current='page']")?.dataset.tab);
const dialog = (p) => p.evaluate(() => { const d = document.querySelector("[role=dialog]"); return d ? d.querySelector("h2")?.textContent : null; });
const notices = (p) => p.evaluate(() => [...document.querySelectorAll("[aria-label=Notices] p")].map((e) => e.textContent));
const focusInDialog = (p) => p.evaluate(() => !!document.activeElement?.closest("[role=dialog]"));
const losses = [];
/** Record where focus is after a dialog closes; recover with Esc (back -> tab rail) if lost. */
const afterClose = async (p, where, mode = "key") => {
  const a = await active(p);
  if (a === "BODY") { losses.push(where); if (mode === "pad") await padPress(p, PAD.B); else await p.keyboard.press("Escape"); }
  return a;
};
const shot = (p, n) => p.screenshot({ path: `${OUT}${n}.png` });
const k = (p, key, n = 1) => (async () => { for (let i = 0; i < n; i++) { await p.keyboard.press(key); await p.waitForTimeout(60); } })();
const toView = async (p, id, mode = "key") => {
  const order = ["overview", "mods", "install", "collections", "requirements", "activity", "settings"];
  for (let i = 0; i < 8 && (await curView(p)) !== id; i++) {
    const d = order.indexOf(id) > order.indexOf(await curView(p));
    if (mode === "pad") await padPress(p, d ? PAD.RB : PAD.LB); else await k(p, d ? "PageDown" : "PageUp");
  }
  expect((await curView(p)) === id, `could not reach ${id}`);
  await p.waitForTimeout(250);
};

// ---------------- Keyboard ----------------
{
  const { page: p, ctx, logs } = await open(browser);
  await step("K overview: initial focus on tab rail", p, async () => { const a = await active(p); expect(a === "button:overview", a); });
  await step("K overview: Purge -> confirm dialog, focus on No, inert background", p, async () => {
    const path = await navTo(p, await goal(p, "Purge"));
    await k(p, "Enter"); await p.waitForTimeout(300);
    expect((await dialog(p)) === "Confirm", "no confirm dialog");
    expect((await active(p)) === "button:No", "focus " + (await active(p)));
    await shot(p, "k-overview-purge-confirm");
    await k(p, "PageDown"); expect((await curView(p)) === "overview", "PageDown changed view under dialog");
    await k(p, "Tab", 3); expect(await focusInDialog(p), "Tab left dialog");
    await k(p, "Tab", 1, true);
    await p.keyboard.press("Shift+Tab"); expect(await focusInDialog(p), "Shift+Tab left dialog");
    for (const d of ["ArrowUp", "ArrowLeft", "ArrowDown", "ArrowRight"]) { await k(p, d); expect(await focusInDialog(p), d + " left dialog"); }
    await k(p, "Escape"); await p.waitForTimeout(300);
    expect((await dialog(p)) === null, "Esc did not close");
    const f = await afterClose(p, "Overview: Purge confirm closed with Esc");
    expect(f === "button:Purge", "focus not restored to Purge after Esc, got " + f);
    return path;
  });
  await step("K overview: Purge confirmed with Yes", p, async () => {
    await navTo(p, await goal(p, "Purge")); await k(p, "Enter"); await p.waitForTimeout(300);
    await navTo(p, await goal(p, "Yes")); await k(p, "Enter"); await p.waitForTimeout(500);
    expect((await dialog(p)) === null, "dialog still open");
    const f = await afterClose(p, "Overview: Purge confirm answered Yes");
    return { focus: f, notices: await notices(p) };
  });
  await step("K overview: Deploy", p, async () => {
    await navTo(p, await goal(p, "Deploy")); await k(p, "Enter"); await p.waitForTimeout(600);
    const f = await afterClose(p, "Overview: Deploy job finished (button busy-disabled)");
    return { focus: f, notices: await notices(p) };
  });
  await step("K overview: Detect Steam games + Esc back to rail", p, async () => {
    await navTo(p, await goal(p, "Detect Steam games")); await k(p, "Enter"); await p.waitForTimeout(500);
    await shot(p, "k-overview-detect");
    const d = await dialog(p); if (d) await k(p, "Escape");
    await k(p, "Escape"); expect((await active(p)) === "button:overview", await active(p));
    return { dialog: d };
  });

  await step("K mods: PageDown, disable SkyUI, move down, details Esc", p, async () => {
    await toView(p, "mods");
    await navTo(p, await goal(p, "Disable SkyUI")); await k(p, "Enter"); await p.waitForTimeout(500);
    expect(await p.evaluate(() => !!document.querySelector("[aria-label='Enable SkyUI']")), "SkyUI not disabled");
    await shot(p, "k-mods-disabled");
    await navTo(p, await goal(p, "Move SkyUI down")); await k(p, "Enter"); await p.waitForTimeout(500);
    const order = await p.evaluate(() => [...document.querySelectorAll("[aria-label='Load order'] > li")].map((l) => l.textContent.slice(0, 30)));
    await navTo(p, await goal(p, "Details")); await k(p, "Enter"); await p.waitForTimeout(400);
    const d = await dialog(p); expect(d, "no details dialog"); await shot(p, "k-mods-details");
    await k(p, "Escape"); await p.waitForTimeout(200);
    expect((await dialog(p)) === null, "details did not close"); expect((await active(p)) === "button:Details", await active(p));
    return { order, d };
  });
  await step("K mods: Conflicts / Updates / Profiles segments", p, async () => {
    const seen = {};
    for (const s of ["Conflicts", "Updates", "Profiles"]) {
      await navTo(p, await goal(p, s)); await k(p, "Enter"); await p.waitForTimeout(400);
      seen[s] = await p.evaluate((s) => document.querySelector(`[aria-pressed=true]`)?.textContent === s, s);
      await shot(p, `k-mods-${s.toLowerCase()}`);
      expect(seen[s], s + " not selected");
    }
    await navTo(p, await goal(p, "Save the current mod set as"));
    await p.keyboard.type("t16"); await k(p, "Enter"); await p.waitForTimeout(400);
    return { seen, notices: await notices(p) };
  });

  await step("K install: type target, Enter -> FOMOD wizard, choose 1K, Next, confirm Yes", p, async () => {
    await k(p, "Escape"); await toView(p, "install");
    await navTo(p, await goal(p, "Mod id, Nexus URL"));
    await p.keyboard.type("266"); await k(p, "Enter"); await p.waitForTimeout(500);
    const d = await dialog(p); expect(d?.startsWith("FOMOD"), "no FOMOD dialog: " + d);
    const f0 = await active(p); expect(f0.includes("2K"), "default focus " + f0);
    await shot(p, "k-install-fomod");
    await k(p, "ArrowUp"); expect((await active(p)).includes("1K"), "ArrowUp " + (await active(p)));
    await k(p, "Enter"); await p.waitForTimeout(150);
    const checked = await p.evaluate(() => [...document.querySelectorAll("[role=dialog] [aria-checked=true],[role=dialog] [aria-pressed=true]")].map((e) => e.textContent.trim()));
    expect(checked.some((c) => c.startsWith("1K")), "1K not selected: " + checked);
    await navTo(p, await goal(p, "Next")); await k(p, "Enter"); await p.waitForTimeout(400);
    expect((await dialog(p)) === "Confirm", "no confirm"); expect((await active(p)) === "button:Yes", await active(p));
    await shot(p, "k-install-confirm");
    await k(p, "Enter"); await p.waitForTimeout(600);
    expect((await dialog(p)) === null, "still dialog");
    const n = await notices(p); await shot(p, "k-install-done");
    await afterClose(p, "Install: confirm answered Yes (keyboard)");
    expect(n.some((t) => /Demo Mod/.test(t)), "no install notice: " + n);
    return n;
  });
  await step("K install: Esc in FOMOD wizard cancels the job", p, async () => {
    await navTo(p, await goal(p, "Mod id, Nexus URL"));
    await p.keyboard.type("267"); await k(p, "Enter"); await p.waitForTimeout(500);
    expect((await dialog(p))?.startsWith("FOMOD"), "no FOMOD");
    await k(p, "Escape"); await p.waitForTimeout(500);
    expect((await dialog(p)) === null, "Esc did not close FOMOD");
    const f = await afterClose(p, "Install: FOMOD cancelled with Esc");
    return { f, notices: await notices(p), focus: await active(p) };
  });
  await step("K install: nxm paste box + search", p, async () => {
    await navTo(p, await goal(p, "nxm link"));
    await p.keyboard.type("nxm://skyrimspecialedition/mods/266/files/1?key=a&expires=1"); await k(p, "Enter"); await p.waitForTimeout(400);
    await navTo(p, await goal(p, "Search")); await p.keyboard.type("sky"); await k(p, "Enter"); await p.waitForTimeout(400);
    await shot(p, "k-install-search");
    return await notices(p);
  });

  for (const v of ["collections", "requirements", "activity"]) {
    await step(`K ${v}: navigate content and back`, p, async () => {
      await k(p, "Escape"); await toView(p, v);
      const info = {};
      if (v === "collections") {
        await navTo(p, await goal(p, "Collection URL, nxm link or slug"));
        await p.keyboard.type("demo-slug"); await k(p, "Enter"); await p.waitForTimeout(500);
        info.dialog = await dialog(p); if (info.dialog) { await shot(p, "k-collections-dialog"); await k(p, "Escape"); }
      }
      if (v === "requirements") {
        await navTo(p, await goal(p, "I have it")); await k(p, "Enter"); await p.waitForTimeout(500);
        info.after = await p.evaluate(() => [...document.querySelectorAll("main button")].map((b) => b.textContent.trim()));
      }
      if (v === "activity") {
        await navTo(p, "pre[tabindex]"); await k(p, "ArrowDown"); info.afterDown = await active(p);
      }
      info.notices = await notices(p);
      await shot(p, `k-${v}`);
      await k(p, "Escape"); expect((await active(p)) === `button:${v}`, "Esc: " + (await active(p)));
      return info;
    });
  }
  await step("K settings: type API key + Enter signs in; config Save", p, async () => {
    await toView(p, "settings");
    await navTo(p, "#nexus-api-key"); await p.keyboard.type("DEMOKEY123"); await k(p, "Enter"); await p.waitForTimeout(600);
    const signed = await p.evaluate(() => document.querySelector("main section")?.textContent);
    expect(/demo/.test(signed), "not signed in");
    await afterClose(p, "Settings: Sign in via Enter (keyboard)");
    await shot(p, "k-settings-signed-in");
    expect(!(await p.evaluate(() => !!document.querySelector("#nexus-api-key")?.value)), "key not cleared");
    return signed.slice(0, 160);
  });
  await step("K settings: Auto-update toggle reachable by arrows (signed in)", p, async () => {
    await k(p, "Escape");
    const path = await navTo(p, "[role=switch]"); await k(p, "Enter"); await p.waitForTimeout(400);
    await afterClose(p, "Settings: Auto-update toggle job finished");
    return { path, state: await p.evaluate(() => document.querySelector("[role=switch]").getAttribute("aria-checked")) };
  });
  await step("K settings: config value edit + Save", p, async () => {
    await navTo(p, await goal(p, "autoupdate-interval"));
    await p.keyboard.press("End"); await p.keyboard.type("2"); 
    await navTo(p, "li:last-of-type button[type=submit]").catch(async () => { await k(p, "ArrowRight"); });
    await k(p, "Enter"); await p.waitForTimeout(400);
    return { focus: await active(p), notices: await notices(p) };
  });
  results.push({ name: "K logs", ok: !logs.console.length && !logs.pageerr.length, info: logs });
  await ctx.close();
}
{
  // Default (signed-out) Settings: is the auto-update toggle reachable?
  const { page: p, ctx } = await open(browser);
  await step("K settings (signed out): Auto-update toggle reachable by arrows", p, async () => {
    await toView(p, "settings");
    return await navTo(p, "[role=switch]");
  });
  await step("K settings (signed out): Down from Keyboard button stays in content", p, async () => {
    await navTo(p, await goal(p, "Keyboard")); await k(p, "ArrowDown");
    const a = await active(p); expect(!a.startsWith("button:settings"), "Down from 'Keyboard' jumped to tab rail " + a); return a;
  });
  await ctx.close();
}

// ---------------- Gamepad ----------------
{
  const { page: p, ctx, logs } = await open(browser, { gamepad: true });
  await p.evaluate(() => window.__padConnect()); await p.waitForTimeout(200);
  await step("G rail: D-pad down/up + RB/LB", p, async () => {
    await padPress(p, PAD.DOWN); const a1 = await active(p);
    await padPress(p, PAD.UP); const a2 = await active(p);
    await padPress(p, PAD.RB); const v1 = await curView(p);
    await padPress(p, PAD.LB); const v2 = await curView(p);
    expect(a1 === "button:mods" && a2 === "button:overview" && v1 === "mods" && v2 === "overview", JSON.stringify({ a1, a2, v1, v2 }));
  });
  await step("G held D-pad repeats (400ms delay, 120ms period)", p, async () => {
    await p.evaluate(() => (window.__pad.buttons[13].pressed = true));
    await p.waitForTimeout(800);
    await p.evaluate(() => (window.__pad.buttons[13].pressed = false));
    await p.waitForTimeout(100);
    const a = await active(p); // 1 + ~4 repeats from overview => ~requirements/activity
    await padPress(p, PAD.B);
    return a;
  });
  await step("G mods: RB, D-pad to Disable SkyUI, A, Details A, B closes", p, async () => {
    await toView(p, "mods", "pad");
    await navTo(p, await goal(p, "Disable SkyUI"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(400);
    expect(await p.evaluate(() => !!document.querySelector("[aria-label='Enable SkyUI']")), "not disabled");
    await navTo(p, await goal(p, "Move SkyUI down"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(400);
    await navTo(p, await goal(p, "Details"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(400);
    expect(await dialog(p), "no details"); await shot(p, "g-mods-details");
    await padPress(p, PAD.RB); expect((await curView(p)) === "mods", "RB switched tab under dialog");
    await padPress(p, PAD.B); await p.waitForTimeout(200);
    expect(!(await dialog(p)), "B did not close"); expect((await active(p)) === "button:Details", await active(p));
    await padPress(p, PAD.B); expect((await active(p)) === "button:mods", await active(p));
  });
  const oskType = async (text) => {
    for (const ch of text) {
      await navTo(p, await goal(p, ch), "pad"); await padPress(p, PAD.A);
    }
  };
  await step("G install: A on target opens OSK, type 266, Done, Install, FOMOD via pad", p, async () => {
    await toView(p, "install", "pad");
    await navTo(p, await goal(p, "Mod id, Nexus URL"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(300);
    const d = await dialog(p); expect(d?.startsWith("Keyboard"), "no OSK: " + d);
    await shot(p, "g-install-osk");
    await oskType("266");
    const typed = await p.evaluate(() => document.querySelector("output[aria-label='Typed text']").textContent);
    await navTo(p, await goal(p, "Done"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(300);
    const val = await p.evaluate(() => document.querySelector("main input").value);
    const f = await afterClose(p, "Install: OSK Done (gamepad)", "pad");
    expect(val === "266", `input value ${val} (typed ${typed})`);
    await navTo(p, "main form button[type=submit]", "pad"); await padPress(p, PAD.A); await p.waitForTimeout(500);
    expect((await dialog(p))?.startsWith("FOMOD"), "no FOMOD");
    await padPress(p, PAD.UP); expect((await active(p)).includes("1K"), await active(p));
    await padPress(p, PAD.A);
    await shot(p, "g-install-fomod");
    await navTo(p, await goal(p, "Next"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(400);
    expect((await dialog(p)) === "Confirm", "no confirm"); await padPress(p, PAD.A); await p.waitForTimeout(500);
    await afterClose(p, "Install: confirm answered A (gamepad)", "pad");
    const n = await notices(p); expect(n.some((t) => /Demo Mod 266/.test(t)), "notice " + n);
    return n;
  });
  await step("G install: B in FOMOD cancels", p, async () => {
    await navTo(p, await goal(p, "Mod id, Nexus URL"), "pad"); await padPress(p, PAD.A);
    await oskType("1"); await navTo(p, await goal(p, "Done"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(200);
    await afterClose(p, "Install: OSK Done (gamepad, 2nd)", "pad");
    await navTo(p, "main form button[type=submit]", "pad"); await padPress(p, PAD.A); await p.waitForTimeout(500);
    expect((await dialog(p))?.startsWith("FOMOD"), "no FOMOD");
    await padPress(p, PAD.B); await p.waitForTimeout(400); expect(!(await dialog(p)), "B did not cancel");
    await afterClose(p, "Install: FOMOD cancelled with B", "pad");
    return await notices(p);
  });
  await step("G settings: A on API key opens OSK, type, Show/Hide masked, Done, Sign in", p, async () => {
    await toView(p, "settings", "pad");
    await navTo(p, "#nexus-api-key", "pad"); await padPress(p, PAD.A); await p.waitForTimeout(300);
    expect((await dialog(p)) === "Keyboard: Nexus API key", "no OSK " + (await dialog(p)));
    await oskType("ab1");
    await navTo(p, "[aria-pressed]", "pad"); await padPress(p, PAD.A); // Shift
    await oskType("C");
    const out = await p.evaluate(() => document.querySelector("output[aria-label='Typed text']").textContent);
    await shot(p, "g-settings-osk");
    await navTo(p, await goal(p, "Done"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(300);
    const val = await p.evaluate(() => document.querySelector("#nexus-api-key").value);
    expect(val === "ab1C", "value " + val);
    const f = await afterClose(p, "Settings: OSK Done (gamepad)", "pad");
    expect(f === "input:Nexus API key", "focus after OSK Done: " + f);
    await navTo(p, await goal(p, "Sign in"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(600);
    const txt = await p.evaluate(() => document.querySelector("main section").textContent);
    await shot(p, "g-settings-signed-in");
    expect(/demo/.test(txt), "not signed in: " + txt.slice(0, 120));
    return { out, txt: txt.slice(0, 120) };
  });
  await step("G settings: Sign in after OSK (continues after focus recovery)", p, async () => {
    if (!(await p.evaluate(() => [...document.querySelectorAll("main button")].some((b) => b.textContent.trim() === "Sign in")))) return "already signed in";
    await navTo(p, await goal(p, "Sign in"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(600);
    const txt = await p.evaluate(() => document.querySelector("main section").textContent);
    expect(/demo/.test(txt), "not signed in: " + txt.slice(0, 120));
    await afterClose(p, "Settings: Sign in via A", "pad");
  });
  await step("G settings: B on OSK cancels without typing", p, async () => {
    await navTo(p, await goal(p, "Change key"), "pad").catch(() => {});
    await padPress(p, PAD.A); await p.waitForTimeout(300);
    const has = await p.evaluate(() => !!document.querySelector("#nexus-api-key"));
    if (!has) return "no key field after Change key";
    await navTo(p, "#nexus-api-key", "pad"); await padPress(p, PAD.A); await p.waitForTimeout(200);
    await oskType("z"); await padPress(p, PAD.B); await p.waitForTimeout(200);
    const v = await p.evaluate(() => document.querySelector("#nexus-api-key").value);
    expect(!(await dialog(p)) && v === "", `dialog ${await dialog(p)} value ${v}`);
  });
  results.push({ name: "G logs", ok: !logs.console.length && !logs.pageerr.length, info: logs });
  await ctx.close();
}
{
  // Fresh state (no "Deploy changes" header button / notices): Settings sign-in by gamepad only.
  const { page: p, ctx } = await open(browser, { gamepad: true });
  await p.evaluate(() => window.__padConnect()); await p.waitForTimeout(200);
  await step("G settings (fresh): OSK entry, Done, Sign in, by pad only", p, async () => {
    await padPress(p, PAD.LB); await p.waitForTimeout(300);
    expect((await curView(p)) === "settings", "LB from overview did not wrap to settings");
    await navTo(p, "#nexus-api-key", "pad"); await padPress(p, PAD.A); await p.waitForTimeout(300);
    for (const ch of "k3y") { await navTo(p, await goal(p, ch), "pad"); await padPress(p, PAD.A); }
    await navTo(p, await goal(p, "Done"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(300);
    const val = await p.evaluate(() => document.querySelector("#nexus-api-key").value);
    const f = await afterClose(p, "Settings (fresh): OSK Done (gamepad)", "pad");
    expect(val === "k3y", "value " + val);
    await navTo(p, await goal(p, "Sign in"), "pad"); await padPress(p, PAD.A); await p.waitForTimeout(600);
    const txt = await p.evaluate(() => document.querySelector("main section").textContent);
    expect(/Signed in as demo/.test(txt), "not signed in");
    await shot(p, "g-settings-fresh-signed-in");
    return { focusAfterDone: f, keyCleared: await p.evaluate(() => !document.querySelector("#nexus-api-key")?.value) };
  });
  await ctx.close();
}
await browser.close();
results.push({ name: "focus lost to <body> after dialog/job", ok: losses.length === 0, info: losses });
writeFileSync(new URL("./flows.json", import.meta.url), JSON.stringify(results, null, 1));
for (const r of results) console.log(r.ok ? "PASS" : "FAIL", r.name, JSON.stringify(r.ok ? r.info ?? "" : { e: r.error, focus: r.focus }).slice(0, 400));
