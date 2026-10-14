// Scenario: open the Layer Stack Manager, load a template, apply, undo.
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
  const setSelect = (sel, value) => b.eval(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(s, ${JSON.stringify(value)}); s.dispatchEvent(new Event("change", { bubbles: true })); })()`);
  await clickText("button", "Layer stack"); await b.wait(500);
  const rows = await b.eval(`document.querySelectorAll(".stack-table tbody tr").length`);
  check("the manager lists the 2-layer stack", rows >= 5, `rows=${rows}`);
  await b.screenshot(join(out, "90-stackup.png"));
  await setSelect('.modal select[aria-label="Template"]', "HDI 1+4+1 (micro-via build-up)"); await b.wait(500);
  check("a template loads into the dialog", (await b.eval(`document.querySelector('.modal select[aria-label="Copper layers"]').value`)) === "6");
  await b.screenshot(join(out, "91-stackup-hdi.png"));
  await clickText(".modal button", "Apply"); await b.wait(700);
  check("applying changes the board to 6 copper layers", (await b.eval(`document.querySelector('select[aria-label="Copper layers"]')?.value ?? [...document.querySelectorAll(".symbol-bar select")][0]?.value`)) === "6");
  const log = async () => { await clickText(".btab", "Console"); await b.wait(200); return text(".bottom-body"); };
  const l1 = await log();
  check("it is one command", l1.includes("Edit layer stack"), l1.replace(/\n/g, " | ").slice(0, 160));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
