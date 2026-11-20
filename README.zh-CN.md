# agent-build

[English](README.md) · 简体中文

给"一台机器上多个 agent、多个 worktree 频繁编译"的场景，以及 CI，用的 Xcode 命令行快速编译。在原来的 `xcodebuild` 命令前加上 `agent-build`：

```bash
cd my-app            # 项目的任意一个 worktree
agent-build xcodebuild build -workspace App.xcworkspace -scheme App -destination 'generic/platform=iOS Simulator'
```

每个项目在本机有一个固定路径的构建槽（一份 git 克隆加一个常驻的 DerivedData）。每次运行把 worktree 的当前状态原样同步进槽，只重写内容变了的文件，然后在槽里运行 `xcodebuild`，并打开 Xcode 的编译缓存。路径固定，Xcode 的增量编译就能从一个 worktree 延续到另一个；共用的编译缓存则让编过一次的东西直接复用，比如切回之前编过的分支。

在一个约 2400 个 Swift 文件的 App 上实测（Xcode 27.2）：新 worktree 首次编译从冷编译的 178 秒降到 45 秒。

## 能看到什么

`xcodebuild` 的输出原样转发，其中槽路径改写成 worktree 路径，报错指向你能打开的文件；逐条的缓存提示不显示。最后几行：

```
agent-build: phases resolve=7.7s graph=5.5s plan=2.0s execute=18.6s
agent-build: precondition P4 not met: App (explicit modules off) — their Swift compiles are not cached
agent-build: top misses: MXAIModule 15, MXAIUIModule 8
agent-build: products=~/.cache/agent-build/slots/my-app-1a2b3c4d/dd/Build/Products (valid until this project's next agent-build run; copy what you keep)
agent-build: ok slot=my-app-1a2b3c4d sync=1.1s build=35.9s cache hit=46 miss=23 log=~/.local/state/agent-build/logs/…
```

- `test` 还会给出 `test results=<路径>.xcresult`，`archive` 给出 `archive=<路径>.xcarchive`。
- `-derivedDataPath`、`-archivePath`、`-resultBundlePath` 会被丢掉并提示：产物留在槽里，要保留就自己复制走。
- 退出码：编译结果原样返回 xcodebuild 的；参数错误为 2；同步槽失败为 3（这时没有编译）。

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
| P8 | 从干净检出、在任意目录下直接运行 `xcodebuild` 就能编过，不依赖被 git 忽略的本地文件 | 用不了：被忽略的文件不在槽里 |

P6 自查：只删掉槽的 DerivedData 里编好的宏插件，再编一次。命中数明显下降，说明插件二进制不可复现。

## 配置

不需要配置。唯一的配置项是共用编译缓存的大小，写在 `$XDG_CONFIG_HOME/agent-build/config.yaml`（默认 `~/.config/agent-build/config.yaml`）：

```yaml
cache_limit: 40G
```

构建槽和缓存在 `$XDG_CACHE_HOME/agent-build/`，随时可以删，删了只会让下次编译变慢；日志在 `$XDG_STATE_HOME/agent-build/logs/`。

## 安装与开发

```bash
just install         # 编译并安装到 ~/.local/bin/agent-build（需要 Rust）
just check           # 格式、lint、单元与集成测试、行数、依赖、用例表核对
just smoke           # 用真 xcodebuild 编样例工程（一两分钟）
```

设计：[docs/design.md](docs/design.md)。验收用例：[docs/cases.md](docs/cases.md)。

## 许可

Apache-2.0 或 MIT。
