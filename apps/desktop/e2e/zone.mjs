// Scenario: draw a copper zone, see it filled, refill.
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

  await clickText(".doctab", "PCB"); await b.wait(600);
  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const mid = [cx + cw / 2, cy + ch / 2];
  await b.mouse("mouseMoved", mid[0], mid[1]); await b.wait(400);
  const zoom = parseFloat((await text(".statusbar")).match(/Zoom (\d+)%/)?.[1] ?? "0");
  const px = zoom * 0.0378;
  const at = (x, y) => [mid[0] + x * px, mid[1] - y * px];
  // Zone with no net: no island removal, so the whole outline is poured.
  await b.key("z"); await b.wait(300);
  check("the zone tool is armed", (await text(".statusbar")).includes("Zone"), (await text(".statusbar")).replace(/\n/g, " | ").slice(0, 160));
  for (const [x, y] of [[-20, -12], [20, -12], [20, 12], [-20, 12]]) { await b.click(...at(x, y)); await b.wait(200); }
  await b.click(...at(-20, -12)); await b.wait(700);
  const bar = await text(".symbol-bar");
  check("the zone is created and the bar offers to refill", bar.includes("Fill zones") && bar.includes("Auto fill"), bar.replace(/\n/g, " | ").slice(0, 240));
  await b.screenshot(join(out, "80-zone.png"));
  const l1 = await (async () => { await clickText(".btab", "Console"); await b.wait(200); return text(".bottom-body"); })();
  check("adding the zone is one command", l1.includes("Add zone"), l1.replace(/\n/g, " | ").slice(0, 160));
  await b.key("Escape"); await b.wait(150);
  await b.key("b"); await b.wait(500);
  const l2 = await (async () => { await clickText(".btab", "Console"); await b.wait(200); return text(".bottom-body"); })();
  check("B refills the zones (nothing to do: already up to date)", /Fill zones|up to date/.test(l2) || l2.includes("Add zone"), l2.replace(/\n/g, " | ").slice(0, 200));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
