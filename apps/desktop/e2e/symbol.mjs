// Scenario: create a symbol in the Symbol Editor (rectangle, pins, units), save it to a library
// and place it in the schematic.
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

  // Symbol Editor tab, new symbol.
  await clickText(".doctab", "Symbol Editor"); await b.wait(400);
  check("the Symbol Editor offers New/Open without pretending", (await text(".stage")).includes("New symbol"));
  await clickText(".stage button", "New symbol"); await b.wait(300);
  await b.eval(`(() => { const i = document.querySelectorAll(".modal input")[1]; i.focus(); i.select(); })()`);
  await typeText(b, "OpAmp");
  await clickText(".modal button", "Create"); await b.wait(900);
  check("the symbol canvas and bar appear", await b.eval(`!!document.querySelector(".symbol-bar") && !!document.querySelector("canvas")`));

  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const mid = [cx + cw / 2, cy + ch / 2];
  await b.mouse("mouseMoved", mid[0], mid[1]); await b.wait(500);
  const zoom = parseFloat((await text(".statusbar")).match(/Zoom (\d+)%/)?.[1] ?? "0");
  const step = zoom * 0.048;
  const at = (gx, gy) => [mid[0] + gx * step, mid[1] - gy * step];
  console.log(`zoom ${zoom}% → ${step.toFixed(1)} px per grid step`);

  // Triangle body from three lines, then pins.
  await b.key("l"); await b.wait(200);
  await b.click(...at(-3, 4)); await b.wait(150);
  await b.click(...at(-3, -4)); await b.wait(150);
  await b.click(...at(4, 0)); await b.wait(150);
  await b.click(...at(-3, 4)); await b.wait(150);
  await b.key("Escape"); await b.wait(150);
  await b.key("p"); await b.wait(200);
  await b.click(...at(-6, 2)); await b.wait(200);
  await b.click(...at(-6, -2)); await b.wait(200);
  await b.key("r"); await b.key("r"); await b.wait(200);
  await b.click(...at(7, 0)); await b.wait(300);
  await b.key("Escape"); await b.wait(150);
  await b.screenshot(join(out, "20-symbol-drawn.png"));
  const cons = async () => { await clickText(".btab", "Console"); await b.wait(200); return text(".bottom-body"); };
  const log = await cons();
  check("each primitive and pin is a command", (log.match(/Add line/g) ?? []).length >= 3 && (log.match(/Add pin/g) ?? []).length >= 3, log.replace(/\n/g, " | ").slice(0, 200));

  // Select the output pin and make it an output via the inspector.
  await b.key("v"); await b.wait(150);
  await b.click(...at(7, 0)); await b.wait(400);
  let inspector = await text(".inspector");
  check("selecting a pin shows its electrical properties", inspector.includes("Electrical type") && inspector.includes("Length"), inspector.replace(/\n/g, " | ").slice(0, 160));
  await b.eval(`(() => { const s = document.querySelector('select[aria-label="Electrical type"]'); const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set; set.call(s, "output"); s.dispatchEvent(new Event("change", { bubbles: true })); })()`);
  await b.wait(500);
  check("changing the pin type runs a command", (await b.eval(`document.querySelector('select[aria-label="Electrical type"]').value`)) === "output");

  // A second unit, then save to the project library.
  await clickText(".symbol-bar button", "+ Unit"); await b.wait(500);
  check("adding a unit switches to it", (await b.eval(`document.querySelector('select[aria-label="Unit shown"]').value`)) === "2");
  await clickText(".symbol-bar button", "Save to library"); await b.wait(300);
  await clickText(".modal button", "Save"); await b.wait(800);
  const msgs = await (async () => { await clickText(".btab", "Messages"); await b.wait(200); return text(".bottom-body"); })();
  check("saving reports the library file", msgs.includes("Saved User:OpAmp"), msgs.replace(/\n/g, " | ").slice(0, 200));

  // Place it in the schematic.
  await clickText(".doctab", "Schematic"); await b.wait(500);
  await b.key("a"); await b.wait(400);
  check("the library symbol is offered to the placement tool", (await text(".symbol-list")).includes("OpAmp"));
  await clickText(".symbol-list li", "OpAmp"); await b.wait(300);
  await b.click(mid[0], mid[1]); await b.wait(500);
  await b.key("Escape"); await b.wait(200);
  await b.screenshot(join(out, "21-symbol-placed.png"));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
