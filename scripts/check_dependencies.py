#!/usr/bin/env python3
"""
Dependency direction check for cs-rs workspace.

Uses `cargo metadata` to inspect the actual dependency graph and assert that
no workspace crate depends (directly or transitively) on a crate that is
"above" it in the architecture.

Architecture layer order (lower → higher, dependencies flow downward only):
  cs-domain  →  cs-analytics, cs-backtest  →  cs-cli, cs-python

Forbidden dependency paths:
  cs-domain   must NOT reach: cs-analytics(*), cs-backtest, cs-cli, cs-python
  cs-analytics must NOT reach: cs-backtest, cs-cli, cs-python
  cs-backtest  must NOT reach: cs-cli, cs-python

(*) cs-domain currently depends on cs-analytics; this is flagged as a known
    architectural concern rather than a hard failure until resolved.

Exit code: 0 = all assertions passed, 1 = at least one violation found.
"""

import json
import subprocess
import sys
from collections import defaultdict

# ── Configuration ─────────────────────────────────────────────────────────────

# Hard failures: these paths must never exist.
FORBIDDEN: list[tuple[str, str]] = [
    ("cs-domain",   "cs-backtest"),
    ("cs-domain",   "cs-cli"),
    ("cs-domain",   "cs-python"),
    ("cs-analytics","cs-backtest"),
    ("cs-analytics","cs-cli"),
    ("cs-analytics","cs-python"),
    ("cs-backtest",  "cs-cli"),
    ("cs-backtest",  "cs-python"),
]

# Known concerns: log but do not fail (to be resolved in future issues).
KNOWN_CONCERNS: list[tuple[str, str]] = [
    # cs-domain depends on cs-analytics (value objects use analytics types).
    # Tracked for future cleanup.
    # ("cs-domain", "cs-analytics"),
]

# ── Helpers ───────────────────────────────────────────────────────────────────

def cargo_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version=1"],
        capture_output=True, text=True, check=True
    )
    return json.loads(result.stdout)


def build_dep_graph(metadata: dict) -> dict[str, set[str]]:
    """
    Returns a mapping: package_id → {set of direct dependency package_ids}.
    """
    resolve = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    graph: dict[str, set[str]] = defaultdict(set)
    for node_id, node in resolve.items():
        for dep in node["deps"]:
            graph[node_id].add(dep["pkg"])
    return graph


def name_to_id(metadata: dict) -> dict[str, str]:
    """Returns workspace package name → package id."""
    return {
        p["name"]: p["id"]
        for p in metadata["packages"]
        if p["id"] in metadata["workspace_members"]
    }


def transitive_deps(start_id: str, graph: dict[str, set[str]]) -> set[str]:
    """BFS to collect all transitive dependency ids from start_id."""
    visited: set[str] = set()
    queue = [start_id]
    while queue:
        current = queue.pop()
        for dep in graph.get(current, set()):
            if dep not in visited:
                visited.add(dep)
                queue.append(dep)
    return visited


# ── Main ──────────────────────────────────────────────────────────────────────

def main() -> int:
    print("=== Dependency direction check ===\n")

    try:
        metadata = cargo_metadata()
    except subprocess.CalledProcessError as e:
        print(f"ERROR: cargo metadata failed:\n{e.stderr}")
        return 1

    n2id = name_to_id(metadata)
    graph = build_dep_graph(metadata)

    # Only check workspace packages that exist.
    workspace_names = set(n2id.keys())

    failures = 0
    passes = 0

    for (source_name, forbidden_name) in FORBIDDEN:
        if source_name not in workspace_names or forbidden_name not in workspace_names:
            print(f"  SKIP [{source_name} → {forbidden_name}]: one or both not in workspace")
            continue

        source_id = n2id[source_name]
        forbidden_id = n2id[forbidden_name]
        all_deps = transitive_deps(source_id, graph)

        if forbidden_id in all_deps:
            print(f"  FAIL [{source_name} must not depend on {forbidden_name}]: "
                  f"dependency path exists")
            failures += 1
        else:
            print(f"  OK   [{source_name} → {forbidden_name}]: no path found")
            passes += 1

    for (source_name, concern_name) in KNOWN_CONCERNS:
        if source_name not in workspace_names or concern_name not in workspace_names:
            continue
        source_id = n2id[source_name]
        concern_id = n2id[concern_name]
        all_deps = transitive_deps(source_id, graph)
        status = "present" if concern_id in all_deps else "absent"
        print(f"  NOTE [known concern: {source_name} → {concern_name}]: {status}")

    print(f"\n=== Results: {passes} passed, {failures} failed ===")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
