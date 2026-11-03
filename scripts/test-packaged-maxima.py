#!/usr/bin/env python3
"""Exercise the packaged CAS and manual through Maximus's headless MCP API."""

import json
import os
import subprocess
import sys
import tempfile


def request(identifier, tool, arguments):
    return json.dumps({
        "jsonrpc": "2.0", "id": identifier, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    })


with tempfile.TemporaryDirectory(prefix="maximus-package-test-") as config:
    env = dict(os.environ, XDG_CONFIG_HOME=config, APPIMAGE_EXTRACT_AND_RUN="1")
    calls = [
        request(1, "evaluate", {"input": "integrate(x^2,x,0,1);"}),
        request(2, "evaluate", {"input": "eigenvalues(matrix([1,0],[0,2]));"}),
        request(3, "help", {"topic": "integrate"}),
    ]
    result = subprocess.run(
        [*sys.argv[1:], "--mcp"], input="\n".join(calls) + "\n",
        text=True, capture_output=True, env=env, timeout=180,
    )
    if result.returncode:
        raise SystemExit(f"Packaged Maximus failed:\n{result.stdout}\n{result.stderr}")
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    if len(replies) != 3:
        raise SystemExit(f"Expected three MCP replies, got: {result.stdout}")
    values = []
    for reply in replies:
        if "error" in reply or reply["result"]["isError"]:
            raise SystemExit(f"Packaged engine/manual failed: {reply}")
        values.append(json.loads(reply["result"]["content"][0]["text"]))
    assert values[0]["outputs"][-1]["text"] == "1/3", values[0]
    assert values[1]["outputs"][-1]["kind"] == "result", values[1]
    assert values[2]["found"] and values[2]["entries"], values[2]
    print("Packaged Maxima integration, share-package loading and help passed")
