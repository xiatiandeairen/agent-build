# agent-build

[English](README.md) · 简体中文

给"一台机器上多个 agent、多个 worktree 频繁编译"的场景，以及 CI，用的 Xcode 命令行快速编译。在原来的 `xcodebuild` 命令前加上 `agb`：

```bash
cd my-app            # 项目的任意一个 worktree
agb xcodebuild build -workspace App.xcworkspace -scheme App -destination 'generic/platform=iOS Simulator'
```

每个项目在本机有一个固定路径的构建槽（一份 git 克隆加一个常驻的 DerivedData）。每次运行把 worktree 的当前状态原样同步进槽，只重写内容变了的文件，然后在槽里运行 `xcodebuild`，并打开 Xcode 的编译缓存。路径固定，Xcode 的增量编译就能从一个 worktree 延续到另一个；项目自己的编译缓存则让编过一次的东西直接复用，比如切回之前编过的分支。

用同一设计的前期原型，在一个约 2400 个 Swift 文件的 App 上实测（Xcode 27.2）：新 worktree 首次编译从冷编译的 178 秒降到 45 秒。当前版本还没在这个 App 上测过。

## 能看到什么

编译期间，xcodebuild 的输出只写进日志文件，终端只有开头一行（给出日志路径，想看过程可以 `tail -f`）。编完后，agb 用 `xcresulttool` 读 Xcode 的结果包（xcresult），依次输出：

```
/Users/me/my-app/App/Login.swift:42:9: error: Cannot find 'session' in scope
agb: test failed: CoreTests/testLogin(): XCTAssertEqual failed: ("1") is not equal to ("2")
agb: phases resolve=7.7s graph=5.5s plan=2.0s execute=18.6s
agb: precondition P4 not met: App (explicit modules off) — their Swift compiles are not cached
agb: top misses: MXAIModule 15, MXAIUIModule 8
agb: results=~/.cache/agent-build/slots/my-app-1a2b3c4d/result.xcresult
agb: failed slot=my-app-1a2b3c4d sync=1.1s build=35.9s cache hit=46 miss=23 errors=1 warnings=12 log=~/.local/state/agent-build/logs/…
```

- 错误按编译器的格式输出，路径是 worktree 里的，编辑器和 agent 可以直接跳过去；警告只计数，完整内容在结果包和日志里。
- 编译失败但读不到结果包时（比如工程文件读不了），改为输出原始输出的最后 30 行。
- `-showBuildSettings`、`-list` 这类只打印信息的命令，原样输出 xcodebuild 的内容。
- 编译成功时还会给出 `products=<路径>`（到这个项目下一次运行 agb 前有效，要保留就复制走），`archive` 给出 `archive=<路径>.xcarchive`。编译失败时只给结果包路径：这时的产物可能是旧的或不存在。
- `-derivedDataPath`、`-archivePath`、`-resultBundlePath`、`-clonedSourcePackagesDirPath` 会被丢掉并提示：产物和包检出都留在槽里，要保留就自己复制走。
- `-workspace`、`-project`、`-xctestrun` 的值如果是 worktree 里的绝对路径（例如 `$PWD/App.xcworkspace`），会改指向槽里的同一份，并提示。
- `-resolvePackageDependencies` 按信息命令处理，解析出的包放进槽里。
- 退出码：编译结果原样返回 xcodebuild 的；参数错误为 64（与 xcodebuild 自己的参数错误相同）；同步槽失败为 125（这时没有编译；125 沿用 `env`、`timeout` 表示"包装器自己失败"的惯例）。

同一项目的两个编译会排队：后到的会输出在等谁，等前一个结束后在热槽上增量编译。不同项目互不等待。机器上同时跑几个编译，交给你的排队工具（例如 [agent-quay](https://github.com/xiatiandeairen/agent-quay)）。CI 用同一条命令，把 runner 配成同一项目不并发即可。

## 设定前提

满足这些前提时，agent-build 又快又正确；不满足时，它不替工程修。

| # | 前提 | 不满足时 |
|---|---|---|
| P1 | 用命令行 `xcodebuild` 编译（Swift，混编 ObjC/C 可以） | 用不了 |
| P2 | 当前正式版 Xcode（已在 27.2 上验证） | 不保证 |
| P3 | 代码在 git 里，子模块本机拿得到 | 用不了 |
| P4 | Swift target 开着 explicit modules | 正确但变慢；按 target 报告 |
| P5 | 构建脚本不改写被跟踪的文件 | 正确但变慢；列出被改写的文件 |
| P6 | 宏插件这类构建工具编出来的结果可复现 | 正确但变慢；见下 |
| P7 | 缓存只在单机内共享 | — |
| P8 | 从干净检出、在任意目录下直接运行 `xcodebuild` 就能编过；编译需要的被 git 忽略的文件在 `.agent-build.yaml` 里声明 | 编译失败；错误里带着缺的文件路径时，agb 点名是哪个被忽略的文件、怎么声明 |

P6 自查：只删掉槽的 DerivedData 里编好的宏插件，再编一次。命中数明显下降，说明插件二进制不可复现。

## 配置

不需要配置。全局唯一的配置项是每个项目编译缓存的上限，写在 `$XDG_CONFIG_HOME/agent-build/config.yaml`（默认 `~/.config/agent-build/config.yaml`）：

```yaml
cache_limit: 20G
```

工程编译需要被 git 忽略的文件时（生成的代码、`Pods/`、编好的 xcframework），在仓库根目录的 `.agent-build.yaml` 里列出来，随项目提交。之后这些文件和被跟踪的文件一样同步：

```yaml
ignored_inputs:
  - rust-core/build/
  - App/Generated.swift
```

Xcode 不执行这个上限，所以某次编译后某个项目的缓存超过 `cache_limit`（二进制单位：1G = 1024³ 字节）时，agb 把它清掉，下次编译重新填充。每个项目的缓存放在它的槽里，删槽就连缓存一起删了。

构建槽和缓存在 `$XDG_CACHE_HOME/agent-build/`，随时可以删，删了只会让下次编译变慢；日志在 `$XDG_STATE_HOME/agent-build/logs/`。

## 安装与开发

```bash
just install         # 编译并安装到 ~/.local/bin/agb（需要 Rust）
just check           # 格式、lint、单元与集成测试、行数、依赖、用例表核对
just smoke           # 用真 xcodebuild 编样例工程（约一分钟，经 aq 借一台 iOS 模拟器）
```

设计：[docs/design.md](docs/design.md)。验收用例：[docs/cases.md](docs/cases.md)。

## 许可

Apache-2.0 或 MIT。
