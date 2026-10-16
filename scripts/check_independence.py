#!/usr/bin/env python3
"""Reject workspace/SDK dependencies before building the standalone app."""
import json
import subprocess

metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], text=True
))
assert len(metadata["workspace_members"]) == 1, "The pet must be a standalone project"
package = next(p for p in metadata["packages"] if p["name"] == "zectrix-agent-pet")
for dep in package["dependencies"]:
    assert dep.get("path") is None, f"Local source dependency: {dep['name']}"
    assert (dep.get("source") or "").startswith("registry+"), f"Non-registry dependency: {dep['name']}"
    assert dep["name"] not in {"zectrix-sdk", "zectrix-cards"}, "Device SDK belongs in Hub"
print("Standalone application: no Hub source, device SDK, local path or git dependencies")
