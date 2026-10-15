// Scenario: start screen → open demo project → select → palette → PCB → light theme.
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { extname, join, resolve } from "node:path";
import { launch } from "./cdp.mjs";

const root = resolve(import.meta.dirname, "..");
const out = join(import.meta.dirname, "out");
mkdirSync(out, { recursive: true });
const demo = process.env.DEMO_PROJECT ?? "/tmp/buturecad-demo";
if (!existsSync(join(demo, "project.buture"))) throw new Error(`run: cargo run -p buturecad-desktop --example demo_project -- ${demo}`);

const bridge = spawn(join(root, "../../target/debug/examples/bridge"), ["7788"], { stdio: "inherit" });
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml" };
const server = createServer((req, res) => {
  const p = req.url === "/" ? "/index.html" : req.url.split("?")[0];
  const f = join(root, "dist", p);
  if (!existsSync(f)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[extname(f)] ?? "application/octet-stream" }).end(readFileSync(f));
}).listen(7790);

const results = [];
const check = (name, ok, extra = "") => { results.push(ok); console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? "  " + extra : ""}`); };

const b = await launch();
try {
  await b.goto("http://127.0.0.1:7790/", readFileSync(join(import.meta.dirname, "shim.js"), "utf8"));
  await b.wait(800);
  await b.screenshot(join(out, "01-start.png"));
  check("start screen is shown without a project", (await b.eval("document.body.innerText")).includes("New project…"));

  await b.eval(`window.__DIALOG_RESULT__ = ${JSON.stringify(demo)}`);
  await b.eval(`[...document.querySelectorAll("button")].find((x) => x.textContent.includes("Open project")).click()`);
  await b.wait(1500);
  await b.screenshot(join(out, "02-opened.png"));
  check("project opens and the canvas renders", await b.eval(`document.querySelector("canvas") && document.querySelector("canvas").width > 100`));
  check("title bar shows the project", (await b.eval("document.title")).startsWith("Demo"));

  const rect = await b.eval(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const [cx, cy, cw, ch] = rect;
  await b.click(683, 297); // the body of R1 in the fitted view
  await b.wait(600);
  await b.screenshot(join(out, "03-clicked.png"));
  const inspector = await b.eval(`document.querySelector(".inspector").innerText`);
  check("clicking R1 selects it and the inspector shows real connectivity", inspector.includes("R1") && inspector.includes("Pin 1") && inspector.includes("+3V3"), inspector.replace(/\n/g, " | ").slice(0, 160));

  await b.key("P", 2 | 8, "KeyP"); // Ctrl+Shift+P
  await b.wait(400);
  await b.eval(`(() => { const i = document.querySelector(".palette input"); i && (i.focus()); })()`);
  await b.screenshot(join(out, "04-palette.png"));
  check("command palette opens with the shortcut", await b.eval(`!!document.querySelector(".palette")`));
  await b.key("Escape");

  await b.eval(`[...document.querySelectorAll(".doctab")].find((x) => x.textContent === "PCB").click()`);
  await b.wait(900);
  await b.screenshot(join(out, "05-pcb.png"));
  check("PCB tab lists real layers", (await b.eval(`document.querySelector(".inspector").innerText`)).includes("F.Cu"));

  await b.eval(`[...document.querySelectorAll(".doctab")].find((x) => x.textContent === "3D Viewer").click()`);
  await b.wait(500);
  check("3D viewer tab shows its controls (it is implemented since stage 27)", (await b.eval("document.body.innerText")).includes("Perspective"));

  await b.eval(`[...document.querySelectorAll(".doctab")].find((x) => x.textContent === "Schematic").click()`);
  await b.wait(500);
  await b.eval(`document.querySelector('[aria-label="Toggle Dark / Light Theme"]').click()`);
  await b.wait(900);
  await b.screenshot(join(out, "06-light.png"));
  check("theme switches", (await b.eval("document.documentElement.dataset.theme")) === "light");

  check("no console errors", b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).length === 0, b.consoleLog.filter((l) => l.startsWith("error") || l.startsWith("EXCEPTION")).join(" | "));
} finally {
  await b.close();
  bridge.kill();
  server.close();
}
console.log(results.every(Boolean) ? "ALL PASSED" : "FAILURES");
process.exit(results.every(Boolean) ? 0 : 1);
