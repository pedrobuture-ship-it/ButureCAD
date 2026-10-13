// Scenario: run ERC on a half-wired circuit, jump to a finding, waive one with a reason.
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
  await clickSym("R");
  await b.wait(300);
  await b.click(...at(0, 0)); await b.wait(250);
  await b.click(...at(12, 0)); await b.wait(250);
  await b.key("Escape"); await b.wait(200);

  // Wire R1.2 to R2.2 only: the other four pins stay unconnected.
  await b.key("w"); await b.wait(200);
  await b.click(...at(0, -3)); await b.wait(150);
  await b.click(...at(6, -5)); await b.wait(150);
  await b.click(...at(12, -3)); await b.wait(300);
  await b.key("Escape"); await b.wait(150);

  await b.key("e", 10); await b.wait(700); // Ctrl+Shift+E (CDP modifiers: Ctrl=2, Shift=8)
  const erc = () => text(".erc");
  let t = await erc();
  check("Ctrl+Shift+E runs ERC and lists real findings", t.includes("Pin not connected") && /Errors 2/.test(t), t.replace(/\n/g, " | ").slice(0, 220));
  check("the tab shows the problem count", (await text(".bottom-tabs")).includes("4"));
  await b.screenshot(join(out, "30-erc.png"));

  // Click a finding: the part is selected and the inspector shows it.
  await clickText(".erc-main", "R1"); await b.wait(600);
  const inspector = await text(".inspector");
  check("clicking a finding selects the offending part", inspector.includes("R1") && inspector.includes("Reference"), inspector.replace(/\n/g, " | ").slice(0, 160));
  await b.screenshot(join(out, "31-erc-focus.png"));

  // Waive the first finding with a reason.
  await clickText(".erc-row .link", "Waive"); await b.wait(300);
  check("waiving needs a reason (the button is disabled while empty)", await b.eval(`document.querySelector(".modal .primary").disabled`));
  await b.eval(`(() => { const t = document.querySelector(".modal textarea"); t.focus(); })()`);
  await typeText(b, "reserved for the test jig");
  await b.wait(200);
  await clickText(".modal button", "Waive"); await b.wait(700);
  t = await erc();
  check("the waived finding stays listed with its reason and is not counted", t.includes("Waived: reserved for the test jig") && /Errors 1/.test(t) && /Waived 1/.test(t), t.replace(/\n/g, " | ").slice(0, 260));
  await b.screenshot(join(out, "32-erc-waived.png"));

  // Any edit makes the report stale until it is run again (here: a value edit in the inspector).
  await b.eval(`(() => { const i = document.querySelector('input[aria-label="value"]'); i.focus(); i.select(); })()`);
  await typeText(b, "4k7");
  await b.key("Enter"); await b.wait(600);
  t = await erc();
  check("editing marks the report as stale", t.includes("run again"));
  await clickText(".erc-bar button", "Run again"); await b.wait(600);
  t = await erc();
  check("running again clears the stale notice and keeps the waiver", !t.includes("run again") && /Waived 1/.test(t), t.replace(/\n/g, " | ").slice(0, 200));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
