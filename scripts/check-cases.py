#!/usr/bin/env python3
"""Check that docs/cases.md and the tests agree.

Usage: scripts/check-cases.py [root]
A test names the cases it guards in a comment on the line right above #[test]
(attributes such as #[ignore] may sit in between), e.g. `// A3, A3.1`.
A test under tests/ that guards no case says why: `// outside the table: <reason>`.
Fails (exit 1) when:
  1. a table row does not have five cells, or its id is malformed or repeated;
  2. a case marked 已实现 with 单元 has no tagged test under crates/ or src/;
  3. a case marked 已实现 with 集成 or 冒烟 has no tagged test under tests/;
  4. a tag names an id that is not in the table;
  5. a test under tests/ has neither a case tag nor an outside-the-table note.
Cases marked 待实现 are counted, not enforced; the work is done when none are left."""
import collections, pathlib, re, sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else pathlib.Path(__file__).resolve().parent.parent).resolve()
ROW_ID = re.compile(r"A[0-9]+(?:\.[0-9]+)?")
TAG_ID = re.compile(r"\bA[0-9]+(?:\.[0-9]+)?\b")
LAYERS = ("单元", "集成", "冒烟", "实测")
STATES = ("待实现", "已实现")
errors = []

cases = collections.OrderedDict()
for n, line in enumerate((root / "docs/cases.md").read_text().splitlines(), 1):
    cells = [c.strip() for c in line.strip().strip("|").split("|")]
    if not line.startswith("| A") or not ROW_ID.fullmatch(cells[0]):
        if line.startswith("| A"):
            errors.append(f"cases.md:{n}: malformed id {cells[0]!r}")
        continue
    if len(cells) != 5:
        errors.append(f"cases.md:{n}: expected 5 cells (id, case, guards against, layer, state), got {len(cells)}")
        continue
    cid, layer, state = cells[0], cells[3], cells[4]
    if cid in cases:
        errors.append(f"cases.md:{n}: id {cid} repeated")
    if not any(k in layer for k in LAYERS):
        errors.append(f"cases.md:{n}: layer {layer!r} is none of {LAYERS}")
    if state not in STATES:
        errors.append(f"cases.md:{n}: state {state!r} is none of {STATES}")
    cases[cid] = (layer, state)

in_crates, in_tests, tagged = collections.defaultdict(list), collections.defaultdict(list), set()
for f in sorted(p for d in ("crates", "src", "tests") for p in (root / d).rglob("*.rs") if "target" not in p.parts):
    rel = f.relative_to(root)
    lines = f.read_text().splitlines()
    for i, line in enumerate(lines):
        if line.strip() != "#[test]":
            continue
        j = i - 1
        while j >= 0 and lines[j].strip().startswith("#["):
            j -= 1
        tag = lines[j].strip() if j >= 0 else ""
        ids = TAG_ID.findall(tag) if tag.startswith("//") else []
        tagged.update(ids)
        for cid in ids:
            (in_tests if rel.parts[0] == "tests" else in_crates)[cid].append(str(rel))
        if rel.parts[0] == "tests" and not ids and not tag.startswith("// outside the table"):
            errors.append(f"{rel}:{i + 1}: test has no case tag and no outside-the-table note")

for cid, (layer, state) in cases.items():
    if state != "已实现":
        continue
    if "单元" in layer and not in_crates.get(cid):
        errors.append(f"{cid}: marked 已实现 with 单元, but no tagged test under crates/ or src/")
    if ("集成" in layer or "冒烟" in layer) and not in_tests.get(cid):
        errors.append(f"{cid}: marked 已实现 with 集成/冒烟, but no tagged test under tests/")
for cid in sorted(tagged - set(cases)):
    errors.append(f"tag {cid} is not in docs/cases.md")

pending = [c for c, (_, s) in cases.items() if s == "待实现"]
manual = [c for c, (l, _) in cases.items() if l == "实测"]
print(f"{len(cases)} cases; {len(cases) - len(pending)} implemented, {len(pending)} pending; {len(manual)} manual (实测)")
for e in errors:
    print("✗", e)
sys.exit(1 if errors else 0)
