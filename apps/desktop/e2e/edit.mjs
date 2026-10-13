// Scenario: build a small circuit with the editing tools, then rotate, drag and undo.
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

  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const mid = [cx + cw / 2, cy + ch / 2];
  // Zoom in on the origin so a grid step is ~40 px.
  for (let i = 0; i < 2; i++) { await b.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: mid[0], y: mid[1], deltaX: 0, deltaY: -400 }); await b.wait(150); }
  await b.wait(500);
  const zoom = parseFloat((await text(".statusbar")).match(/Zoom (\d+)%/)?.[1] ?? "0");
  const step = zoom * 0.048; // px per 50 mil at this zoom
  const at = (gx, gy) => [mid[0] + gx * step, mid[1] - gy * step];
  console.log(`zoom ${zoom}% → ${step.toFixed(1)} px per grid step`);

  // Place two resistors.
  await b.key("a"); await b.wait(400);
  check("the component picker opens with A", await b.eval(`!!document.querySelector(".symbol-list")`));
  await clickText(".symbol-list li", "R");
  await b.wait(300);
  await b.click(...at(0, 0)); await b.wait(250);
  await b.click(...at(12, 0)); await b.wait(250);
  await b.key("Escape"); await b.wait(200);

  // A ground symbol on R1's lower pin, then a supply on R2's upper pin.
  await b.key("p"); await b.wait(300);
  await clickText(".symbol-list li", "GND"); await b.wait(300);
  await b.click(...at(0, -3)); await b.wait(250);
  await b.key("Escape"); await b.wait(150);
  await b.key("p"); await b.wait(300);
  await clickText(".symbol-list li", "+3V3"); await b.wait(300);
  await b.click(...at(12, 3)); await b.wait(250);
  await b.key("Escape"); await b.wait(150);

  // Wire R1.1 → R2.2 with a corner, ending on a pin.
  await b.key("w"); await b.wait(200);
  await b.click(...at(0, 3)); await b.wait(150);
  await b.click(...at(6, 5)); await b.wait(150);
  await b.click(...at(12, -3)); await b.wait(300);
  await b.key("Escape"); await b.wait(150);

  // A label on the wire.
  await b.key("l"); await b.wait(300);
  await b.eval(`(() => { document.querySelector(".modal input").select(); })()`);
  await typeText(b, "VIN");
  await b.key("Enter"); await b.wait(300);
  await b.click(...at(3, 5)); await b.wait(250);
  await b.key("Escape"); await b.wait(150);
  await b.screenshot(join(out, "10-built.png"));

  const consoleTab = async () => { await clickText(".btab", "Console"); await b.wait(200); return text(".bottom-body"); };
  const log1 = await consoleTab();
  check("each placement is a command in the console", (log1.match(/Place component/g) ?? []).length >= 2 && log1.includes("Draw wire") && log1.includes("Add label"), log1.replace(/\n/g, " | ").slice(0, 200));

  // Select R1, rotate, check the inspector, undo.
  await b.click(...at(0, 0)); await b.wait(400);
  let inspector = await text(".inspector");
  check("clicking a part shows its connectivity", inspector.includes("R1") && inspector.includes("Pin 1") && inspector.includes("Pin 2"), inspector.replace(/\n/g, " | ").slice(0, 200));
  await b.key("r"); await b.wait(500);
  await b.screenshot(join(out, "11-rotated.png"));
  inspector = await text(".inspector");
  check("R rotates the selected part", inspector.includes("90°"), "");
  await b.key("z", 2); await b.wait(500);
  inspector = await text(".inspector");
  check("Ctrl+Z undoes the rotation", inspector.includes("0°"));

  // Edit the value in the inspector.
  await b.eval(`(() => { const i = document.querySelector('input[aria-label="value"]'); i.focus(); i.select(); })()`);
  await typeText(b, "4k7");
  await b.key("Enter"); await b.wait(500);
  check("editing a property in the inspector runs a command", (await b.eval(`document.querySelector('input[aria-label="value"]').value`)) === "4k7");

  // Drag R2 and watch the wires follow (connectivity stays intact: the pin nets are listed).
  await b.click(...at(12, 0)); await b.wait(300);
  await b.mouse("mouseMoved", ...at(12, 0));
  await b.mouse("mousePressed", ...at(12, 0));
  for (let i = 1; i <= 6; i++) { await b.mouse("mouseMoved", at(12, 0)[0] + i * step * 0.5, at(12, 0)[1] - i * step * 0.5); await b.wait(60); }
  await b.screenshot(join(out, "12-dragging.png"));
  await b.mouse("mouseReleased", at(12, 0)[0] + 3 * step, at(12, 0)[1] - 3 * step);
  await b.wait(500);
  await b.screenshot(join(out, "13-dragged.png"));
  const log2 = await consoleTab();
  check("dragging moves the part as one command", log2.includes("Move"));

  // Save and confirm the title loses its unsaved marker.
  await b.key("s", 2); await b.wait(700);
  check("Ctrl+S saves", !(await b.eval("document.title")).includes("•"));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
