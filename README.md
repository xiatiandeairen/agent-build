# agent-build

English · [简体中文](README.zh-CN.md)

Fast command-line Xcode builds for machines where many agents work in many worktrees, and for CI.
Put `agent-build` in front of your `xcodebuild` command:

```bash
cd my-app            # any worktree of the project
agent-build xcodebuild build -workspace App.xcworkspace -scheme App -destination 'generic/platform=iOS Simulator'
```

Each project gets one build slot at a fixed path (a git clone plus a persistent DerivedData). Every
run copies the worktree's exact state into the slot, rewriting only files whose content changed, and
runs `xcodebuild` there with Xcode's compilation cache turned on. Because the path never changes,
Xcode's incremental build carries over from one worktree to the next, and the shared cache serves
anything already compiled once, such as switching back to a branch built earlier.

Measured on a 2400-file app with Xcode 27.2: a new worktree's first build went from 178 s (cold) to
45 s.

## What you see

`xcodebuild`'s output is passed through with slot paths rewritten to worktree paths, so errors
point at files you can open. Per-task cache remarks are kept out of the terminal. At the end:

```
agent-build: phases resolve=7.7s graph=5.5s plan=2.0s execute=18.6s
agent-build: precondition P4 not met: App (explicit modules off) — their Swift compiles are not cached
agent-build: top misses: MXAIModule 15, MXAIUIModule 8
agent-build: products=~/.cache/agent-build/slots/my-app-1a2b3c4d/dd/Build/Products (valid until this project's next agent-build run; copy what you keep)
agent-build: ok slot=my-app-1a2b3c4d sync=1.1s build=35.9s cache hit=46 miss=23 log=~/.local/state/agent-build/logs/…
```

- `test` also prints `test results=<path>.xcresult`; `archive` prints `archive=<path>.xcarchive`.
- `-derivedDataPath`, `-archivePath` and `-resultBundlePath` are dropped with a notice: outputs stay
  in the slot, and you copy what you keep.
- Exit code: xcodebuild's own; 2 for a usage error; 3 when syncing the slot failed (nothing was built).

Two builds of the same project take turns: the second prints whom it is waiting for and then builds
incrementally in the warm slot. Different projects never wait for each other. How many builds run at
once on the machine is left to your queue (for example [agent-quay](https://github.com/xiatiandeairen/agent-quay)).
CI uses the same command; configure runners not to run two jobs of the same project at once.

## Preconditions

agent-build is fast and correct when these hold; it does not fix a project that breaks them.

| # | Precondition | If not met |
|---|---|---|
| P1 | Built with command-line `xcodebuild` (Swift, mixed ObjC/C fine) | not usable |
| P2 | The current Xcode release (verified with 27.2) | not guaranteed |
| P3 | Code in git; submodules available locally | not usable |
| P4 | Swift targets use explicit modules | correct but slower; reported per target |
| P5 | Build scripts do not rewrite tracked files | correct but slower; the files are reported |
| P6 | Build tools such as macro plugins are reproducible | correct but slower; see below |
| P7 | The cache is shared on one machine only | — |
| P8 | A clean checkout builds with plain `xcodebuild` from any directory: nothing comes from git-ignored local files | not usable: ignored files are not in the slot |

P6 self-check: delete only the built macro plugin in the slot's DerivedData and build again. If the
hit count drops, the plugin binary is not reproducible.

## Configuration

None is needed. The only setting is the size of the shared compilation cache, in
`$XDG_CONFIG_HOME/agent-build/config.yaml` (default `~/.config/agent-build/config.yaml`):

```yaml
cache_limit: 40G
```

Slots and the cache live in `$XDG_CACHE_HOME/agent-build/` and can be deleted at any time (the next
build is just slower); logs live in `$XDG_STATE_HOME/agent-build/logs/`.

## Install and develop

```bash
just install         # builds and installs ~/.local/bin/agent-build (needs Rust)
just check           # format, lint, unit and integration tests, size, dependencies, case table
just smoke           # the real xcodebuild on a sample project (a minute or two)
```

Design: [docs/design.md](docs/design.md) (Chinese). Acceptance cases: [docs/cases.md](docs/cases.md).

## License

Apache-2.0 or MIT.
