// Minimal ButureCAD agent client in TypeScript (Node 22.6+ runs it directly; no dependencies).
//
//   node client.ts /path/to/buturecad /path/to/project
//
// Starts `buturecad agent serve <project>` and speaks JSON-RPC 2.0 over its stdin/stdout.
// It inspects the project and PROPOSES a decoupling capacitor; it cannot apply anything.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const [binary, project] = process.argv.slice(2);
const child = spawn(binary, ["agent", "serve", project], { stdio: ["pipe", "pipe", "inherit"] });
const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
let nextId = 0;

async function rpc(method: string, params: object = {}): Promise<any> {
  const id = ++nextId;
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  const { value } = await lines.next();
  const reply = JSON.parse(value);
  if (reply.error) throw new Error(reply.error.message);
  return reply.result;
}

async function tool(name: string, args: object = {}): Promise<any> {
  const result = await rpc("tools/call", { name, arguments: args });
  if (result.isError) throw new Error(result.content[0].text);
  return result.structuredContent;
}

try {
  const info = await rpc("initialize", { clientInfo: { name: "example-typescript" } });
  const tools = (await rpc("tools/list")).tools;
  console.log(`connected to ${info.serverInfo.name}, ${tools.length} tools`);

  const components = (await tool("get_components")).components;
  console.log("components:", components.map((c: { reference: string }) => c.reference).join(", "));

  const proposal = await tool("propose_changes", {
    title: "Add a bypass capacitor between nets A and B",
    actions: [
      { action: "create_component", reference: "C1", symbol: "Buture:C", value: "100nF", footprint: "0402" },
      { action: "connect", net: "A", pins: [{ reference: "C1", pin: "1" }] },
      { action: "connect", net: "B", pins: [{ reference: "C1", pin: "2" }] },
      { action: "place", reference: "C1", x: 14000000, y: 14000000 },
    ],
  });
  const drc = proposal.validation.drc;
  console.log(
    `proposal ${proposal.id} is ${proposal.status}: ${proposal.validation.diff.entries.length} changes, ` +
      `DRC errors ${drc.errors_before} -> ${drc.errors_after}`,
  );
  console.log(`review it with: buturecad agent show ${project} ${proposal.id}`);
} finally {
  child.stdin.end();
}
