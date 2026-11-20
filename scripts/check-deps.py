#!/usr/bin/env python3
"""Zero third-party dependencies, and core stays pure.

Usage: scripts/check-deps.py [root]
Every dependency in every Cargo.toml must be a path dependency inside the workspace,
and crates/agent-build-core must have no dependencies at all. Exits 1 on any violation."""
import pathlib, re, sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else pathlib.Path(__file__).resolve().parent.parent).resolve()
SECTION = re.compile(r"^\[(?:target\.[^\]]+\.)?((?:dev-|build-)?dependencies)\]\s*$")
errors = []

for manifest in sorted(root.glob("**/Cargo.toml")):
    if "target" in manifest.relative_to(root).parts:
        continue
    rel = manifest.relative_to(root)
    is_core = rel.parts[:2] == ("crates", "agent-build-core")
    section = None
    for n, line in enumerate(manifest.read_text().splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("["):
            m = SECTION.match(stripped)
            section = m.group(1) if m else None
            continue
        if section is None or not stripped or stripped.startswith("#"):
            continue
        name = stripped.split("=", 1)[0].strip()
        if is_core:
            errors.append(f"{rel}:{n}: core must not depend on anything ({name})")
            continue
        path = re.search(r'path\s*=\s*"([^"]+)"', stripped)
        if not path:
            errors.append(f"{rel}:{n}: {name} is not a workspace path dependency")
        elif not (manifest.parent / path.group(1)).resolve().is_relative_to(root):
            errors.append(f"{rel}:{n}: {name} points outside the workspace")

for e in errors:
    print("✗", e)
print("dependencies ok" if not errors else f"{len(errors)} dependency problem(s)")
sys.exit(1 if errors else 0)
