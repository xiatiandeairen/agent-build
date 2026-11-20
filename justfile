# One entry point: format, lint, unit and integration tests, file size, dependencies, case table.
# Run `just check` after every change; the change is done only when it is all green.
check: fmt clippy test size deps cases

fmt:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# Every .rs file ≤ 400 lines (functions ≤ 60 lines are enforced by clippy)
size:
    scripts/check-size.sh

# No third-party crates; core depends on nothing
deps:
    scripts/check-deps.py

# docs/cases.md and the tagged tests agree
cases:
    scripts/check-cases.py

# The real xcodebuild on the sample projects in tests/fixtures (a few minutes; borrows an iOS
# simulator through aq). Run before every delivery, queued: `aq run -- just smoke`
smoke:
    scripts/smoke.sh

# Install to ~/.local/bin/agb
install:
    cargo install --path . --root ~/.local --locked --force
