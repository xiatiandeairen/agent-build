#!/bin/bash
# Every .rs file stays at or under 400 lines; functions stay at or under 60 lines (clippy too_many_lines).
set -euo pipefail
cd "$(dirname "$0")/.."
bad=0
while IFS= read -r f; do
  n=$(wc -l < "$f" | tr -d ' ')
  if [ "$n" -gt 400 ]; then echo "over 400 lines: $f ($n), split by responsibility"; bad=1; fi
done < <(find src crates tests -name '*.rs' 2>/dev/null)
exit $bad
