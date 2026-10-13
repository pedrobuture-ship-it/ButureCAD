// Scenario: route a track with a via, change width, undo.
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
  check("a new project is created and its canvas shown", (await b.eval("document.title")).startsWith("Untitled"));

  await clickText(".doctab", "Footprint Editor"); await b.wait(300);
  await clickText(".stage button", "Generate a package"); await b.wait(300);
  await clickText(".modal button", "Generate"); await b.wait(900);
  await clickText(".doctab", "PCB"); await b.wait(600);
  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const mid = [cx + cw / 2, cy + ch / 2];
  await b.mouse("mouseMoved", mid[0], mid[1]); await b.wait(400);
  const zoom = parseFloat((await text(".statusbar")).match(/Zoom (\d+)%/)?.[1] ?? "0");
  const px = zoom * 0.0378;
  const at = (x, y) => [mid[0] + x * px, mid[1] - y * px];
  await b.key("a"); await b.wait(400);
  await clickSym2();
  async function clickSym2() { await b.eval(`[...document.querySelectorAll(".symbol-list li")].find((x) => x.textContent.includes("SOIC-8"))?.click()`); await b.wait(300); }
  await b.click(...at(-15, 0)); await b.wait(300);
  await b.click(...at(15, 0)); await b.wait(300);
  await b.key("Escape"); await b.wait(200);

  // Route from pad 1 of the left part (local -2.7, 1.905) to pad 8 of the right one (+2.7, 1.905).
  await b.key("x"); await b.wait(200);
  check("the route tool is armed", (await b.eval("document.querySelector('.statusbar').innerText")).includes("Route"));
  await b.click(...at(-15 + 2.7, 1.905)); await b.wait(300);
  check("a route starts on the pad and the bar shows it", (await text(".symbol-bar")).includes("Routing"), (await text(".symbol-bar")).replace(/\n/g, " | ").slice(0, 200));
  await b.click(...at(-5, 6)); await b.wait(250);
  await b.mouse("mouseMoved", ...at(0, 6)); await b.wait(200);
  await b.key("v"); await b.wait(300);
  check("V places a via and the route continues on B.Cu", (await text(".symbol-bar")).includes("B.Cu") && (await text(".symbol-bar")).includes("1 via"), (await text(".symbol-bar")).replace(/\n/g, " | ").slice(0, 220));
  await b.click(...at(8, 6)); await b.wait(250);
  await b.mouse("mouseMoved", ...at(8, 6)); await b.wait(200);
  await b.screenshot(join(out, "70-routing.png"));
  await b.key("Enter"); await b.wait(500);
  await b.screenshot(join(out, "71-routed.png"));
  const log = async () => { await clickText(".btab", "Console"); await b.wait(200); return text(".bottom-body"); };
  const l1 = await log();
  check("the route is one command", l1.includes("Route track with vias"), l1.replace(/\n/g, " | ").slice(0, 200));
  check("the route ended", !(await text(".symbol-bar")).includes("Routing"));

  await b.key("Escape"); await b.wait(150); await b.key("Escape"); await b.wait(150);
  await b.key("a", 2); await b.wait(400);
  const insp = await text(".inspector");
  check("tracks, via and footprints are selectable objects", /\d+ objects selected/.test(insp), insp.replace(/\n/g, " | ").slice(0, 120));
  await b.key("z", 2); await b.wait(500);
  const l2 = await log();
  check("undo removes the route", l2.includes("Undo: Route track with vias"), l2.replace(/\n/g, " | ").slice(0, 200));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
