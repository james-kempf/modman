// axe-core scan of the prompt dialogs (confirm, FOMOD wizard, OSK) and the Mods details dialog.
import { writeFileSync } from "node:fs";
import { chromium, AxeBuilder, open } from "./lib.mjs";
const b = await chromium.launch(); const { page: p } = await open(b);
const scan = async () => (await new AxeBuilder({ page: p }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"]).analyze()).violations.map((x) => ({ id: x.id, impact: x.impact, n: x.nodes.length, targets: x.nodes.slice(0, 3).map((n) => n.target.join(" ")) }));
const out = {};
await p.click("text=Purge"); await p.waitForTimeout(300); out.confirm = await scan(); await p.keyboard.press("Escape");
await p.click("[data-tab=mods]"); await p.waitForTimeout(300); await p.click("text=Details >> nth=0"); await p.waitForTimeout(300); out.details = await scan(); await p.keyboard.press("Escape");
await p.click("[data-tab=install]"); await p.waitForTimeout(300);
await p.fill("main input >> nth=0", "266"); await p.keyboard.press("Enter"); await p.waitForTimeout(400); out.fomod = await scan(); await p.keyboard.press("Escape"); await p.waitForTimeout(300);
await p.click("[data-tab=settings]"); await p.waitForTimeout(300); await p.click("text=Keyboard"); await p.waitForTimeout(300); out.osk = await scan();
writeFileSync(new URL("./axe-dialogs.json", import.meta.url), JSON.stringify(out, null, 1)); console.log(JSON.stringify(out));
await b.close();
