#!/usr/bin/env python3
"""Minimal ButureCAD agent client (Python standard library only).

    python3 client.py /path/to/buturecad /path/to/project

Starts `buturecad agent serve <project>` and speaks JSON-RPC 2.0 over its stdin/stdout.
It inspects the project and PROPOSES a decoupling capacitor. It cannot apply anything: the user
runs `buturecad agent show|apply` (or reviews it in the app).
"""
import json
import subprocess
import sys


class Agent:
    def __init__(self, binary, project):
        self.proc = subprocess.Popen(
            [binary, "agent", "serve", project],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
        )
        self.next_id = 0

    def rpc(self, method, params=None):
        self.next_id += 1
        msg = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}}
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        reply = json.loads(self.proc.stdout.readline())
        if "error" in reply:
            raise RuntimeError(reply["error"]["message"])
        return reply["result"]

    def tool(self, name, **arguments):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments})
        if result["isError"]:
            raise RuntimeError(result["content"][0]["text"])
        return result["structuredContent"]

    def close(self):
        self.proc.stdin.close()
        self.proc.wait()


def main():
    binary, project = sys.argv[1], sys.argv[2]
    agent = Agent(binary, project)
    try:
        info = agent.rpc("initialize", {"clientInfo": {"name": "example-python"}})
        tools = agent.rpc("tools/list")["tools"]
        print(f"connected to {info['serverInfo']['name']}, {len(tools)} tools")

        components = agent.tool("get_components")["components"]
        print("components:", ", ".join(c["reference"] for c in components))

        proposal = agent.tool(
            "propose_changes",
            title="Add a bypass capacitor between nets A and B",
            actions=[
                {"action": "create_component", "reference": "C1", "symbol": "Buture:C",
                 "value": "100nF", "footprint": "0402"},
                {"action": "connect", "net": "A", "pins": [{"reference": "C1", "pin": "1"}]},
                {"action": "connect", "net": "B", "pins": [{"reference": "C1", "pin": "2"}]},
                {"action": "place", "reference": "C1", "x": 14000000, "y": 14000000},
            ],
        )
        drc = proposal["validation"]["drc"]
        print(f"proposal {proposal['id']} is {proposal['status']}: "
              f"{len(proposal['validation']['diff']['entries'])} changes, "
              f"DRC errors {drc['errors_before']} -> {drc['errors_after']}")
        print(f"review it with: buturecad agent show {project} {proposal['id']}")
    finally:
        agent.close()


if __name__ == "__main__":
    main()
