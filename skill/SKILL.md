---
name: agb
description: agb 在固定的构建槽里编译 Xcode 工程和 Rust 项目：在 worktree 里跑 xcodebuild 的 build、test、archive，或 cargo 的 build、test、run、check 时使用，写成 `agb xcodebuild ...`、`agb cargo ...` 代替直接调用；编译慢、缓存命中低、要给工程提速时也用。
---

# agb

agb 把当前 worktree 的内容同步进这个项目固定路径的槽（一份 git 克隆，加上常驻的 DerivedData 或 cargo 的 target 目录），在槽里跑 xcodebuild 或 cargo，所以换 worktree、切回编过的提交都能复用上次的结果。参数照原工具写；输出位置归 agb 管。

## 用哪个命令

| 要做的事 | 命令 |
| --- | --- |
| xcodebuild 的 build、test、archive、build-for-testing、test-without-building | `agb xcodebuild <参数>` |
| `-showBuildSettings`、`-list` 这类只打印信息的 | `agb xcodebuild <参数>`，输出原样给出 |
| `-exportArchive` | 直接 `xcodebuild`：它不编译，agb 不接 |
| cargo 的 build、check、test、bench、run、doc、clippy、rustc、clean | `agb cargo <参数>` |
| cargo 的 fmt、fix、`clippy --fix`、add、remove、update、install 等改文件或不编译的 | 在 worktree 里直接 `cargo`：交给 agb 会以 64 退出 |

本机装了 aq 时，经它排队：`aq run -- agb xcodebuild <参数>`、`aq run -- agb cargo <参数>`（排队和等待按 aq skill）。

`agb cargo clean` 清空的是这个项目所有 worktree 共用的槽里的 target 目录，之后每个 worktree 都要从头编；编译结果不对时先看错误行，不靠 clean。

## 跑一次编译

1. 在 worktree 或它的子目录里，照平时的参数在 `xcodebuild` 或 `cargo` 前面加 `agb`。输出位置参数不用传，传了会被丢掉并提示：xcodebuild 的 `-derivedDataPath`、`-archivePath`、`-resultBundlePath`、`-clonedSourcePackagesDirPath`，cargo 的 `--target-dir`、`--message-format`（`CARGO_TARGET_DIR` 也不起作用）。完成判据：拿到退出码。
2. 按「输出和退出码」处理 stderr 里的行：先看编译器格式的错误和 `agb: test failed:`，再看最后一行汇总。`cargo run`、`bench` 的程序输出原样在 stdout。完成判据：汇总行是 `agb: ok`；或每个错误、每个失败的测试都已在 worktree 里修掉并重新编过。
3. 要用产物时，从 `agb: products=`、`agb: archive=` 给的路径取。它们在这个 worktree 的 `.agents/build/output/` 下（xcodebuild 在 `Products/`；cargo 在 `target/`，路径与 target 目录里相同，如 `target/debug/libfoo.a`），只会被这个 worktree 下一次产出同类东西的运行替换；要长期留的复制出来。cargo 只有 `build`、`rustc` 交产物，只交本项目的库和程序（`.a`、`.dylib`、可执行文件），`check`、`test` 不动上次的产物。完成判据：拿到了要用的产物路径。

改代码只在 worktree 里改：槽每次运行都会被同步成 worktree 的样子，在槽里改的东西下一次就没了。

## 输出和退出码

xcodebuild、cargo 的完整输出写进日志，终端只有 agb 整理过的行，路径都已换回 worktree 里的路径。只标了一个工具的行只有它会出。

| 输出 | 意思 | 下一步 |
| --- | --- | --- |
| `<路径>:<行>:<列>: error: ...` | 编译错误 | 到这个 worktree 路径修 |
| `agb: test failed: <测试>: <原因>` | 测试失败 | 修代码或测试；xcodebuild 的完整信息在 `results=` 的结果包里，cargo 的在 `log=` 的日志里 |
| `agb: no result bundle to read; the last N lines ...` | xcodebuild：没生成结果包（如工程读不了），后面跟原始输出的末尾 | 从这些行找原因；不够就读 `log=` 的日志 |
| `agb: the last N lines of cargo's output:` | cargo：失败了但没有编译错误或失败的测试（如构建脚本、链接失败），后面跟 cargo 输出的末尾 | 同上 |
| `agb: waiting for the slot: <谁>` | 同一项目的另一个编译正在用槽 | 等它结束；agb 随后在热槽上增量编译 |
| `agb: building in slot ...; xcodebuild's full output: <日志>`（cargo 是 `cargo's full output`） | 开始编译 | 想看进度就 `tail -f` 这个日志 |
| `agb: rebuilt: <包>, ...` | cargo：这次重编了本项目哪些包 | 时间花在这些包上；同一提交在别的 worktree 编过时应当没有这行 |
| `agb: phases resolve=… graph=… plan=… execute=…` | xcodebuild：各阶段耗时 | 要提速时按 [speed.md](speed.md) 解读 |
| `agb: precondition P4/P5/P8 not met: ...` | 工程不满足某条前提，后面点名 target 或文件 | P8 会让编译失败，按行内提示在仓库根的 `.agents/build/config.yaml` 的 `ignored_inputs` 里声明（这个文件提交进仓库）；P4、P5 只是变慢，按 [speed.md](speed.md) 处理 |
| `agb: top misses: <target> <次数>, ...` | xcodebuild：缓存未命中最多的 target | 首次编译或改了这些 target 时属正常；切回编过的提交仍很多时，按 [speed.md](speed.md) 查 |
| `agb: ignored -derivedDataPath ...`、`agb: ignored --target-dir ...` 等 | 输出位置参数被丢掉 | 无需处理 |
| `agb: -workspace <路径> is in the worktree; building the slot's copy ...`（cargo 是 `--manifest-path`） | worktree 里的绝对路径改指向槽里的同一份 | 无需处理 |
| `agb: RUSTC_WRAPPER cleared: ...` 等 | cargo：环境里设的 rustc wrapper（sccache 等）在槽里不生效，复用只由 Cargo 判断 | 无需处理 |
| `agb: cache ... exceeded cache_limit ... and was cleared` | xcodebuild：编译缓存超上限，已清空 | 下一次编译会慢一些，自动回填 |
| `agb: ok\|failed slot=… sync=…s build=…s … errors=… warnings=… log=…` | 汇总，最后一行；中间 xcodebuild 是 `cache hit=… miss=…`，cargo 是 `units fresh=… rebuilt=…`（复用和重编的编译单元数） | 警告只计数，要看内容读结果包或日志 |

| 退出码 | 意思 | 下一步 |
| --- | --- | --- |
| 0 | 编译成功 | 继续 |
| 64 | agb 的参数写法错，或 cargo 子命令不在槽里跑（与 xcodebuild 自己的参数错误同码） | 按提示改调用，或直接用 cargo |
| 125 | 同步槽失败，这次没有编译 | 把错误行转告用户；常见原因是子模块本机拿不到 |
| 127 | 找不到 xcodebuild 或 cargo | 告诉用户检查 Xcode 安装和 `xcode-select`，或 Rust 工具链 |
| 128+N | agb 收到信号 N 被中断，已停掉整组子进程 | 需要就重跑 |
| 其他 | xcodebuild、cargo 自己的退出码（cargo 编译失败、测试失败是 101） | 按错误行处理 |

## 出错时

| 现象 | 怎么办 |
| --- | --- |
| 找不到 `agb` | 告诉用户在 agent-build 仓库里跑 `just install` |

## 给工程提速

Xcode 工程编译慢、缓存命中低、或输出里有 `precondition` 行时，读 [speed.md](speed.md)：怎么从 agb 的输出判断时间花在哪，以及工程层面哪些改法有实测收益、哪些不值得做。
