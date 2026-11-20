#!/bin/bash
# The real xcodebuild on the sample projects. Borrows one iOS simulator from aq for A13 and
# returns it however the tests end. Run it through the queue: `aq run -- just smoke`.
set -euo pipefail
hold=$(aq hold ios-sim --wait 600 | tail -1)
id=$(printf '%s' "$hold" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
trap 'aq release "$id" >/dev/null' EXIT
status=$(printf '%s' "$hold" | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])')
if [ "$status" != held ]; then echo "could not borrow a simulator: $hold" >&2; exit 1; fi
sim=$(printf '%s' "$hold" | python3 -c 'import json,sys; print(json.load(sys.stdin)["devices"][0]["id"])')
AGENT_BUILD_SMOKE_SIM="$sim" cargo test --test smoke -- --ignored --test-threads 2 "$@"
