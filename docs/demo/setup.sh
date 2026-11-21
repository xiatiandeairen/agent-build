#!/bin/bash
# Prepares an isolated demo under $1 (default /tmp/agb-demo): the sample app from tests/fixtures
# in a git repo "app" plus a second worktree "app-feature", its own agent-build cache and config,
# and $AGB_BIN_DIR (if set) first on PATH. Your real build slots are not touched.
set -euo pipefail
demo=${1:-/tmp/agb-demo}
here=$(cd "$(dirname "$0")" && pwd)
rm -rf "$demo" && mkdir -p "$demo"
cp -R "$here/../../tests/fixtures/SampleApp" "$demo/app"
cd "$demo/app"
git init -q && git add -A
git -c core.hooksPath=/dev/null -c user.name=demo -c user.email=demo@example.com commit -qm "chore: init"
git worktree add -q ../app-feature -b feature
cat > "$demo/env" <<ENV
export PATH=${AGB_BIN_DIR:+$AGB_BIN_DIR:}\$PATH BASH_SILENCE_DEPRECATION_WARNING=1
export XDG_CACHE_HOME=$demo/cache XDG_CONFIG_HOME=$demo/config
export PS1='\W \$ '
ENV
echo "demo ready: source $demo/env && cd $demo/app"
