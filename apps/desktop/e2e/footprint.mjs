// Scenario: generate a SOIC footprint, edit a pad in the inspector, add silkscreen, save to a library.
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

  await clickText(".doctab", "Footprint Editor"); await b.wait(400);
  check("the Footprint Editor offers real actions", (await text(".stage")).includes("Generate a package"));
  await clickText(".stage button", "Generate a package"); await b.wait(400);
  check("the generator dialog lists the families", (await text(".modal")).includes("SOIC") && (await text(".modal")).includes("BGA"));
  await clickText(".modal button", "Generate"); await b.wait(1200);
  check("the generated footprint opens on the canvas", await b.eval(`!!document.querySelector(".symbol-bar") && !!document.querySelector("canvas")`));
  const bar = await text(".symbol-bar");
  check("the bar names the footprint", bar.includes("Generated:SOIC-8_3.9mm_P1.27mm"), bar.replace(/\n/g, " | ").slice(0, 160));
  await b.wait(500);
  await b.screenshot(join(out, "40-footprint-soic.png"));

  const inspector0 = await text(".inspector");
  check("the inspector shows the footprint, no warnings and the layers", inspector0.includes("Pads") && inspector0.includes("Courtyard") && !inspector0.includes("no courtyard"), inspector0.replace(/\n/g, " | ").slice(0, 200));

  // Add an SMD pad of the next number and edit its shape in the inspector.
  const [cx, cy, cw, ch] = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  await b.key("s"); await b.wait(200);
  await b.click(cx + cw / 2, cy + ch / 2 + 40); await b.wait(500);
  let inspector = await text(".inspector");
  check("placing a pad selects it and shows its properties", inspector.includes("Shape") && inspector.includes("Width (mm)") && inspector.includes("Pad 9"), inspector.replace(/\n/g, " | ").slice(0, 200));
  await b.eval(`(() => { const s = document.querySelector('select[aria-label="Shape"]'); const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set; set.call(s, "round_rect"); s.dispatchEvent(new Event("change", { bubbles: true })); })()`);
  await b.wait(500);
  inspector = await text(".inspector");
  check("choosing a rounded rectangle exposes its corner radius", inspector.includes("Corner radius"), inspector.replace(/\n/g, " | ").slice(0, 200));
  await b.key("Escape"); await b.wait(150);
  await b.screenshot(join(out, "41-footprint-edited.png"));

  // Other generators.
  await clickText(".symbol-bar button", "Generate"); await b.wait(300);
  await b.eval(`(() => { const s = document.querySelector('select[aria-label="Package family"]'); const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set; set.call(s, "bga"); s.dispatchEvent(new Event("change", { bubbles: true })); })()`);
  await b.wait(200);
  await clickText(".modal button", "Generate"); await b.wait(1200);
  await b.screenshot(join(out, "42-footprint-bga.png"));
  check("a BGA is generated too", (await text(".symbol-bar")).includes("BGA-64"));

  // Save to the project library.
  await clickText(".symbol-bar button", "Save to library"); await b.wait(300);
  await clickText(".modal button", "Save"); await b.wait(800);
  await clickText(".btab", "Messages"); await b.wait(200);
  const msgs = await text(".bottom-body");
  check("saving reports the library file", msgs.includes("Saved Generated:BGA-64"), msgs.replace(/\n/g, " | ").slice(0, 260));
  await clickText(".symbol-bar button", "Open"); await b.wait(400);
  const list = await text(".symbol-list");
  check("the open dialog lists project and library footprints", list.includes("Generated:BGA-64") && list.includes("Generated:SOIC-8"), list.replace(/\n/g, " | ").slice(0, 200));
  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
