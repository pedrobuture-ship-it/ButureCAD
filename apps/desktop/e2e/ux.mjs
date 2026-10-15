// Scenario: keyboard workflow (stage 46): previous tool, repeat, palette recents, shortcut sheet, context menu.
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";
import { launch, typeText } from "./cdp.mjs";

const root = resolve(import.meta.dirname, "..");
const out = join(import.meta.dirname, "out");
mkdirSync(out, { recursive: true });
const parent = mkdtempSync(join(tmpdir(), "buturecad-e2e-proj-"));

const bridge = spawn(join(root, "../../target/debug/examples/bridge"), ["7788"], { stdio: "inherit" });
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };
const server = createServer((req, res) => {
  const f = join(root, "dist", req.url === "/" ? "/index.html" : req.url.split("?")[0]);
  if (!existsSync(f)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[extname(f)] ?? "application/octet-stream" }).end(readFileSync(f));
}).listen(7790);

const results = [];
const check = (name, ok, extra = "") => { results.push(ok); console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? "  " + extra : ""}`); };

const b = await launch();
const text = (sel) => b.eval(`document.querySelector(${JSON.stringify(sel)})?.innerText ?? ""`);
const clickSym = (name) => b.eval(`[...document.querySelectorAll(".symbol-list li")].find((x) => x.querySelector("strong")?.textContent === ${JSON.stringify(name)})?.click()`);
const clickText = (sel, label) => b.eval(`[...document.querySelectorAll(${JSON.stringify(sel)})].find((x) => x.textContent.includes(${JSON.stringify(label)}))?.click()`);

try {
  await b.goto("http://127.0.0.1:7790/", readFileSync(join(import.meta.dirname, "shim.js"), "utf8"));
  await b.wait(600);
  await b.eval(`window.__DIALOG_RESULT__ = ${JSON.stringify(parent)}`);
  await clickText("button", "New project");
  await b.wait(300);
  await clickText(".modal button", "Browse");
  await b.wait(300);
  await clickText(".modal button", "Create");
  await b.wait(1200);
  check("a new project is created", (await b.eval("document.title")).startsWith("Untitled"));

  const activeTool = () => b.eval(`document.querySelector(".tp-btn.active")?.getAttribute("aria-label") ?? ""`);
  await b.key("w"); await b.wait(250);
  const wire = await activeTool();
  await b.key("v"); await b.wait(250);
  const select = await activeTool();
  check("W then V switch tools", wire !== select && wire.length > 0 && select.length > 0, `${wire} -> ${select}`);
  await b.key("q"); await b.wait(250);
  check("Q goes back to the previous tool", (await activeTool()) === wire, await activeTool());
  await b.key("q"); await b.wait(250);
  check("Q again alternates", (await activeTool()) === select, await activeTool());

  // Shortcut sheet.
  await b.key("F1"); await b.wait(300);
  const sheet = await b.eval(`document.querySelector(".modal")?.innerText ?? ""`);
  check("F1 shows the generated shortcut sheet", sheet.includes("Keyboard Shortcuts") && sheet.includes("Repeat Last Command") && sheet.includes("Ctrl+Enter"));
  await b.key("Escape"); await b.wait(200);
  check("Escape closes it", !(await b.eval(`!!document.querySelector(".modal")`)));

  // Palette: recent commands first.
  await b.key("p", 2 | 8); await b.wait(300); // Ctrl+Shift+P
  const first = await b.eval(`[...document.querySelectorAll(".palette li")].slice(0, 2).map((x) => x.innerText.replace(/\\s+/g, " ")).join(" | ")`);
  check("the palette lists recent commands first", first.includes("Recent"), first);
  await b.key("Escape"); await b.wait(200);

  // Context menu: repeat is offered, mode specific items.
  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  await b.click(cx + cw / 2, cy + ch / 2, { button: "right", buttons: 2, clickCount: 1 });
  await b.wait(300);
  const ctx = await b.eval(`document.querySelector(".ctx")?.innerText ?? ""`);
  check("right click opens the context menu with Repeat Last Command", ctx.includes("Repeat Last Command") && ctx.includes("Rotate"), ctx.replace(/\s+/g, " ").slice(0, 80));
  await b.key("Escape"); await b.wait(200);

  // Footprint suggestions (stage 53): place a resistor, select it, the inspector offers footprints.
  const clickSym = (name) => b.eval(`[...document.querySelectorAll(".symbol-list li")].find((x) => x.querySelector("strong")?.textContent === ${JSON.stringify(name)})?.click()`);
  await b.key("a"); await b.wait(400);
  await clickSym("R"); await b.wait(300);
  const [px, py, pw, ph] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  await b.click(px + pw / 2, py + ph / 2); await b.wait(300);
  await b.key("Escape"); await b.wait(200);
  await b.key("v"); await b.wait(200);
  await b.click(px + pw / 2, py + ph / 2); await b.wait(500);
  const insp = await b.eval(`document.querySelector(".inspector")?.innerText ?? ""`);
  check("the inspector shows the part and the footprint suggestion list", insp.includes("Footprint") && /Chip_0603/.test(insp), insp.replace(/\s+/g, " ").slice(0, 200));
  await b.screenshot(join(out, "ux-footprint-suggestions.png"));
  await b.eval(`[...document.querySelectorAll(".fp-item")].find((x) => x.textContent.includes("Chip_0603"))?.click()`);
  await b.wait(500);
  const fpValue = await b.eval(`document.querySelector('input[aria-label="footprint"]')?.value ?? ""`);
  check("picking a suggestion assigns the footprint", fpValue === "Std:Chip_0603", fpValue);

  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
