// Injected before the app loads. Stand-in for Tauri's JS bridge.
(() => {
  const BRIDGE = window.__BRIDGE__ || "http://127.0.0.1:7788";
  let cb = 0;
  window.__DIALOG_RESULT__ = window.__DIALOG_RESULT__ ?? null;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: () => ++cb,
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    async invoke(cmd, args) {
      if (cmd.startsWith("plugin:dialog|")) return window.__DIALOG_RESULT__;
      if (cmd === "plugin:event|listen") return ++cb;
      if (cmd.startsWith("plugin:")) return null;
      const res = await fetch(`${BRIDGE}/invoke/${cmd}`, { method: "POST", body: JSON.stringify(args ?? {}) });
      if (!res.ok) throw JSON.parse(await res.text());
      if ((res.headers.get("content-type") || "").includes("octet-stream")) return res.arrayBuffer();
      return res.json();
    },
  };
})();
