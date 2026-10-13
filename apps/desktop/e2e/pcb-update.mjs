// Scenario: Update PCB from Schematic: preview, apply, undo.
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

  await clickText(".doctab", "Footprint Editor"); await b.wait(300);
  await clickText(".stage button", "Generate a package"); await b.wait(300);
  await clickText(".modal button", "Generate"); await b.wait(900);
  await clickText(".doctab", "Schematic"); await b.wait(500);
  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const mid = [cx + cw / 2, cy + ch / 2];
  await b.key("a"); await b.wait(400);
  await clickSym("R"); await b.wait(300);
  await b.click(mid[0] - 80, mid[1]); await b.wait(300);
  await b.click(mid[0] + 80, mid[1]); await b.wait(300);
  await b.key("Escape"); await b.wait(200);
  // Assign the footprint to R2 (selected via click) through the inspector.
  await b.click(mid[0] + 80, mid[1]); await b.wait(400);
  await b.eval(`(() => { const i = document.querySelector('input[aria-label="footprint"]'); i.focus(); i.select(); })()`);
  await typeText(b, "Generated:SOIC-8_3.9mm_P1.27mm");
  await b.key("Enter"); await b.wait(500);

  await clickText(".doctab", "PCB"); await b.wait(500);
  await clickText(".symbol-bar button", "Update from schematic"); await b.wait(700);
  const modal = await text(".modal");
  check("the preview lists what would be added and warns about the part without footprint", modal.includes("Add R2") && modal.includes("no footprint"), modal.replace(/\n/g, " | ").slice(0, 260));
  check("nothing has been applied yet", (await text(".symbol-bar")).includes("0 footprints"));
  await b.screenshot(join(out, "60-pcb-update-preview.png"));
  await clickText(".modal button", "Apply changes"); await b.wait(800);
  check("applying adds the footprint", (await text(".symbol-bar")).includes("1 footprint"), (await text(".symbol-bar")).replace(/\n/g, " | "));
  await clickText(".symbol-bar button", "Update from schematic"); await b.wait(600);
  check("a second preview has nothing to apply", await b.eval(`document.querySelector(".modal .primary").disabled`));
  await b.key("Escape"); await b.wait(200);
  await b.key("z", 2); await b.wait(500);
  check("one undo removes the whole update", (await text(".symbol-bar")).includes("0 footprints"));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
