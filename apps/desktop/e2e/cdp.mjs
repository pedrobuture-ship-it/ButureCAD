// Minimal Chrome DevTools Protocol client (Node's built-in WebSocket).
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export async function launch({ port = 9333, width = 1500, height = 950 } = {}) {
  const profile = mkdtempSync(join(tmpdir(), "buturecad-e2e-"));
  const proc = spawn("chromium", ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, `--window-size=${width},${height}`, "--no-first-run", "--disable-gpu", "about:blank"], { stdio: "ignore" });
  let targets;
  for (let i = 0; i < 50; i++) {
    try {
      targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      if (targets.some((t) => t.type === "page")) break;
    } catch { /* not up yet */ }
    await new Promise((r) => setTimeout(r, 200));
  }
  const page = targets?.find((t) => t.type === "page");
  if (!page) throw new Error("chromium did not start");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let id = 0;
  const pending = new Map();
  const consoleLog = [];
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
    } else if (msg.method === "Runtime.consoleAPICalled") {
      consoleLog.push(`${msg.params.type}: ${msg.params.args.map((a) => a.value ?? a.description).join(" ")}`);
    } else if (msg.method === "Runtime.exceptionThrown") {
      consoleLog.push(`EXCEPTION: ${msg.params.exceptionDetails.exception?.description ?? msg.params.exceptionDetails.text}`);
    }
  };
  const send = (method, params = {}) => new Promise((resolve, reject) => { const i = ++id; pending.set(i, { resolve, reject }); ws.send(JSON.stringify({ id: i, method, params })); });
  await send("Page.enable");
  await send("Runtime.enable");
  const api = {
    send,
    consoleLog,
    async goto(url, preload) {
      if (preload) await send("Page.addScriptToEvaluateOnNewDocument", { source: preload });
      await send("Page.navigate", { url });
      await new Promise((r) => setTimeout(r, 800));
    },
    async eval(expr) {
      const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
      return r.result.value;
    },
    async screenshot(file) {
      const r = await send("Page.captureScreenshot", { format: "png" });
      (await import("node:fs")).writeFileSync(file, Buffer.from(r.data, "base64"));
    },
    async mouse(type, x, y, extra = {}) {
      await send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1, ...extra });
    },
    async click(x, y, extra = {}) {
      await api.mouse("mouseMoved", x, y);
      await api.mouse("mousePressed", x, y, extra);
      await api.mouse("mouseReleased", x, y, extra);
    },
    async key(key, mods = 0, code) {
      await send("Input.dispatchKeyEvent", { type: "keyDown", key, code: code ?? key, modifiers: mods, windowsVirtualKeyCode: key.length === 1 ? key.toUpperCase().charCodeAt(0) : 0 });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key, code: code ?? key, modifiers: mods });
    },
    async wait(ms) { await new Promise((r) => setTimeout(r, ms)); },
    async close() { try { await send("Browser.close"); } catch { /* closing */ } ws.close(); proc.kill(); },
  };
  return api;
}
