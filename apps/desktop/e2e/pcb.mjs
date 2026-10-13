// Scenario: PCB editor: board outline, place footprints, flip, align, layers.
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
  check("the PCB view has a bar with the stackup and an outline warning", (await text(".symbol-bar")).includes("Copper layers") && (await text(".symbol-bar")).includes("no board outline"));
  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const mid = [cx + cw / 2, cy + ch / 2];
  await b.mouse("mouseMoved", mid[0], mid[1]); await b.wait(400);
  const zoom = parseFloat((await text(".statusbar")).match(/Zoom (\d+)%/)?.[1] ?? "0");
  const mmpx = zoom * 0.0378; // px per mm
  const at = (x, y) => [mid[0] + x * mmpx, mid[1] - y * mmpx];
  console.log(`zoom ${zoom}% → ${mmpx.toFixed(1)} px/mm`);

  await b.key("o"); await b.wait(200);
  await b.click(...at(-40, -25)); await b.wait(200);
  await b.click(...at(40, 25)); await b.wait(400);
  check("the outline is drawn as four Edge.Cuts segments", !(await text(".symbol-bar")).includes("no board outline"));

  await b.key("a"); await b.wait(400);
  check("the footprint picker lists the generated SOIC", (await text(".symbol-list")).includes("SOIC-8"));
  await clickText(".symbol-list li", "SOIC-8"); await b.wait(300);
  await b.click(...at(-20, 8)); await b.wait(300);
  await b.click(...at(0, 0)); await b.wait(300);
  await b.click(...at(24, -6)); await b.wait(300);
  await b.key("Escape"); await b.wait(200);
  await b.screenshot(join(out, "50-pcb-placed.png"));
  check("three footprints are on the board", (await text(".symbol-bar")).includes("3 footprints"), (await text(".symbol-bar")).replace(/\n/g, " | "));

  await b.click(...at(0, 0)); await b.wait(400);
  let inspector = await text(".inspector");
  check("selecting shows the footprint's properties", inspector.includes("Reference") && inspector.includes("Locked") && inspector.includes("Side"), inspector.replace(/\n/g, " | ").slice(0, 160));
  await b.key("f"); await b.wait(500);
  inspector = await text(".inspector");
  check("F flips it to the back", inspector.includes("Back (flipped)") || (await b.eval(`document.querySelector('select[aria-label="Side"]')?.value`)) === "true");
  await b.screenshot(join(out, "51-pcb-flipped.png"));
  await b.key("k"); await b.wait(400);
  check("K locks it", (await b.eval(`document.querySelector('select[aria-label="Locked"]')?.value`)) === "true");
  await b.key("a", 2); await b.wait(300);
  await clickText(".btab", "Console"); await b.wait(200);
  const log = await text(".bottom-body");
  check("every edit is a command in the console", log.includes("Place footprint") && log.includes("Flip") && log.includes("Draw board outline") && log.includes("Lock"), log.replace(/\n/g, " | ").slice(0, 240));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
