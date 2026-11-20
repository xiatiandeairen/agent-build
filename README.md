# agent-build

English · [简体中文](README.zh-CN.md)

Fast command-line Xcode builds for machines where many agents work in many worktrees, and for CI.
Put `agb` in front of your `xcodebuild` command:

```bash
cd my-app            # any worktree of the project
agb xcodebuild build -workspace App.xcworkspace -scheme App -destination 'generic/platform=iOS Simulator'
```

Each project gets one build slot at a fixed path (a git clone plus a persistent DerivedData). Every
run copies the worktree's exact state into the slot, rewriting only files whose content changed, and
runs `xcodebuild` there with Xcode's compilation cache turned on. Because the path never changes,
Xcode's incremental build carries over from one worktree to the next, and the project's compilation
cache serves anything already compiled once, such as switching back to a branch built earlier.

Measured with an earlier prototype of the same design on a 2400-file app with Xcode 27.2: a new
worktree's first build went from 178 s (cold) to 45 s. This version has not been measured on that
app yet.

## What you see

While xcodebuild runs, its output goes to a log file; the terminal shows one line with the log's
path (`tail -f` it to watch). Afterwards agb reads Xcode's result bundle with `xcresulttool` and
prints, in order:

```
/Users/me/my-app/App/Login.swift:42:9: error: Cannot find 'session' in scope
agb: test failed: CoreTests/testLogin(): XCTAssertEqual failed: ("1") is not equal to ("2")
agb: phases resolve=7.7s graph=5.5s plan=2.0s execute=18.6s
agb: precondition P4 not met: App (explicit modules off) — their Swift compiles are not cached
agb: top misses: MXAIModule 15, MXAIUIModule 8
agb: results=~/.cache/agent-build/slots/my-app-1a2b3c4d/result.xcresult
agb: failed slot=my-app-1a2b3c4d sync=1.1s build=35.9s cache hit=46 miss=23 errors=1 warnings=12 log=~/.local/state/agent-build/logs/…
```

- Errors are printed the way compilers print them, with worktree paths, so editors and agents can
  jump to them; warnings are counted, and listed in full in the result bundle and the log.
- When a failed build leaves no result bundle (for example, the project cannot be read), the last
  30 lines of the raw output are printed instead.
- Commands that only print information, such as `-showBuildSettings` and `-list`, show xcodebuild's
  output as is.
- A successful build also prints `products=<path>` (valid until this project's next agb run; copy
  what you keep), and `archive` prints `archive=<path>.xcarchive`. A failed build points only at
  its result bundle: its products may be stale or missing.
- `-derivedDataPath`, `-archivePath`, `-resultBundlePath` and `-clonedSourcePackagesDirPath` are
  dropped with a notice: outputs and package checkouts stay in the slot, and you copy what you keep.
- An absolute path into the worktree given to `-workspace`, `-project` or `-xctestrun` (such as
  `$PWD/App.xcworkspace`) is pointed at the slot's copy, with a notice.
- `-resolvePackageDependencies` is treated like the information commands; its checkouts land in the slot.
- Exit code: xcodebuild's own; 64 for a usage error (as xcodebuild itself); 125 when syncing the slot failed (nothing was built; 125 is the usual "the wrapper itself failed" code of `env` and `timeout`).

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
| P8 | A clean checkout builds with plain `xcodebuild` from any directory; git-ignored files the build needs are declared in `.agent-build.yaml` | the build fails; when the error mentions the missing ignored file's path, agb names it and how to declare it |

P6 self-check: delete only the built macro plugin in the slot's DerivedData and build again. If the
hit count drops, the plugin binary is not reproducible.

## Configuration

None is needed. The only global setting is the size limit of each project's compilation cache, in
`$XDG_CONFIG_HOME/agent-build/config.yaml` (default `~/.config/agent-build/config.yaml`):

```yaml
cache_limit: 20G
```

A project whose build needs git-ignored files (generated code, `Pods/`, a built xcframework) lists them in `.agent-build.yaml` at the repository root, committed with the project. They are then synced like tracked files:

```yaml
ignored_inputs:
  - rust-core/build/
  - App/Generated.swift
```

Xcode does not enforce this limit, so agb clears a project's cache when a build leaves it over
`cache_limit` (binary units: 1G = 1024³ bytes); the next build refills it. Each project's cache lives in
its slot, so deleting a slot also drops its cache.

Slots and the cache live in `$XDG_CACHE_HOME/agent-build/` and can be deleted at any time (the next
build is just slower); logs live in `$XDG_STATE_HOME/agent-build/logs/`.

## Install and develop

```bash
just install         # builds and installs ~/.local/bin/agb (needs Rust)
just check           # format, lint, unit and integration tests, size, dependencies, case table
just smoke           # the real xcodebuild on the sample projects (about a minute; borrows an iOS simulator through aq)
```

Design: [docs/design.md](docs/design.md) (Chinese). Acceptance cases: [docs/cases.md](docs/cases.md).

## License

Apache-2.0 or MIT.
