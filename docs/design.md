# agent-build 方案（iOS/macOS 与 Rust 极速编译）


**状态：** 已实现（Rust，本仓库）。验收用例与每条的状态见 [cases.md](cases.md)；除 A10（my-app 实测，等接入方在 `.agent-build/config.yaml` 里声明 rust-core 产物）和 A28（rust-sdk 实测）外全部通过。cargo 的部分见 [Cargo（agb cargo）](#cargoagb-cargo)。

**一句话方案：** 每个项目在本机有一个固定路径的构建槽（git 克隆 + 常驻 DerivedData），agent 从任何 worktree 编译时，agent-build 把代码同步进这个槽再调用 xcodebuild；路径固定让 Xcode 的增量编译跨 worktree 继续有效，每个槽带一份自己的编译缓存，来回切换分支时编过的部分直接命中。

**例子：** agent 在新 worktree 里运行 `agb xcodebuild build -scheme App`。agent-build 用这个项目的槽（昨天编过 develop），切到这次的提交，只重写内容变了的文件，再让 Xcode 增量编译。前期原型实测首编从冷编译 178 秒降到 45 秒（当前版本待 A10 复测）。

## 目标、范围与前提

在设定的前提下，让命令行编译最快且结果正确；使用方是同一台机器上多个项目、每个项目多个 worktree、多个 agent 的频繁编译，以及 CI。长期做多端，这一期只做 iOS 和 macOS。验收数字先不定。

### 目标

| # | 做成后谁能看到什么 |
| --- | --- |
| G1 | agent 在同一个 worktree 里改完再编，只做这次改动带来的工作 |
| G2 | 新 worktree、新会话第一次编译，接近同一提交上的日常再编，不从零开始 |
| G3 | 多个 agent 同时编译互不干扰：同一项目同一时刻只有一个编译在用它的槽，其余等它编完再在热槽上增量；不同项目各用各的槽 |
| G4 | CI 上用 agent-build 编译和跑单测时同样复用热槽；新分支第一次推送不从零开始编 |
| G5 | 正确性是设计目标：只有影响结果的输入全部相同才复用，结果和从零编译完全一致；靠设计保证，不靠事后兜底 |
| G6 | 每次编译都能看到复用了多少、时间花在哪；P4、P5 没满足时指出是哪条前提、涉及哪些 target 或文件；P6 只列出未命中最多的 target，不推断原因，文档给自查方法 |
| G7 | 标准的 iOS/macOS Xcode 工程不写配置就能用，只有偏离默认的地方才需要配置 |
| G8 | 判断逻辑和外部操作分层，判断逻辑能单独测试；每个对外行为都有用例，一条命令知道有没有改坏；单文件 ≤ 400 行、单函数 ≤ 60 行；不引入第三方依赖 |

### 范围

| 做 | 不做，以及原因 |
| --- | --- |
| iOS/macOS 的 Swift 工程，混编的 ObjC/C 照常参与复用 | Android、Electron：长期方向，另起卡片 |
| Rust 工程的 cargo 编译（`agb cargo`，见 [Cargo](#cargoagb-cargo) 一节） | cargo 之外的出包步骤（绑定生成、lipo、打 xcframework）：接入方拿 output 里的产物自己做（D4） |
| 本机 agent 的命令行编译，以及 CI | Xcode IDE 里的开发体验 |
| 前提满足与否的说明（G6） | 替项目修前提（依赖二进制化、修 pre-action、关 dSYM、约束检查）：接入方自己的事 |
| 单机内共享复用 | 远程共享缓存；排队和资源管控（交给 aq）；定期全量对照这类事后兜底（不符合 G5）；CI 如何安装 agent-build（落地阶段再定） |

### 设定前提

| # | 前提 | 不满足时 |
| --- | --- | --- |
| P1 | 用命令行 xcodebuild 编译 iOS 或 macOS 的 Swift 工程（混编 ObjC/C 可以） | 用不了 |
| P2 | 当前正式版 Xcode（2026-11-19 为 27.2），升级后重新验证 | 不保证 |
| P3 | 代码在 git 仓库里，子模块在本机拿得到 | 用不了 |
| P4 | 参与编译的 Swift target 都开着 explicit modules | 照常正确编译，复用变少，输出里说明 |
| P5 | 编译流程里的脚本和代码生成器不改写被 git 跟踪的文件；生成物放构建目录，必须放仓库里的内容没变时不重写 | 同上 |
| P6 | 编译结果只取决于源码和工具链；宏插件这类构建工具编出来的结果可复现 | 照常正确编译，复用变少；不自动检测，见 G6 |
| P7 | 缓存只在单机内共享 | — |
| P8 | 从干净的 git 检出、在任意目录下直接运行 xcodebuild 就能编过（不写死 worktree 的绝对路径，不往构建目录之外写东西）；编译需要的东西由工程自己在 SwiftPM 解析或构建步骤里准备；依赖被 git 忽略的本地文件（生成代码、`Pods/`、编好的 xcframework）时，在仓库根目录的 `.agent-build/config.yaml` 里声明（未提交、未忽略的改动会照常同步） | 没声明的被忽略文件不在槽里，编译失败；错误里带着它的路径时，agb 点名它是哪个被忽略的文件、该怎么声明（只报符号名、不报路径的错误无从点名） |

### 核心例子

| 目标 | 情形 | 期望效果 |
| --- | --- | --- |
| G1 典型 | 在 Feature 模块里改了一个 Swift 函数的实现，再编 | 只重做这个模块受影响的工作，其他模块不重编 |
| G1 易误解 | 改了一个被很多模块引用的 ObjC 公开头 | 引用它的模块全部重编，这是正确行为 |
| G2 典型 | 从本机已经编过的 develop 最新提交开一个新 worktree，第一次编译 | 接近在那个提交上做一次增量编译 |
| G2 易误解 | 新 worktree 落在相差 50 个提交的分支上 | 没变的部分复用，变了的部分正常编 |
| G3 | 两个 agent 同时编译同一项目 | 后一个等前一个编完，再在同一个热槽上增量；等待时输出在等谁 |
| G4 | 新分支第一次推送到 CI | 不从零开始编 |
| G5 典型 | 在同一个槽里 A → B → A 来回切换提交 | 每次的结果都和从零编译该提交完全一致 |
| G5 易误解 | 改了 ObjC 头之后，增量编译报"找不到符号" | 设计上就不该出现；"出错后自动清掉重编"不算满足 G5 |
| G6 | 一个不满足 P4 的工程编完一次 | 看到命中多少、各阶段耗时，以及哪些 target 没开 explicit modules |
| G7 | 一个普通的 Swift iOS 工程，没有配置文件 | 直接 `agb xcodebuild build -scheme App` 就能用 |
| G8 | 修改同步规则 | 用例马上指出改坏了哪种情况；不引入新依赖 |

## 取舍优先级与总览

取舍按这个顺序裁决，后文每个选择都标了依据哪一条：

1. **正确（G5）**：只靠 Xcode 自己的输入追踪，工具不另造判断、不兜底。
2. **堵根源、克制**：一件事只由一个机制负责；能用 Xcode 和 git 现成的就不自己造；删掉后没有目标受损的就删。
3. **快（G1～G4）**。
4. **看得见（G6）**。
5. **零配置（G7）**。

G8 代码质量是贯穿的约束，不参与排序。

### 每个目标由哪个机制负责

| 目标 | 负责的机制 | 怎么做到 |
| --- | --- | --- |
| G1 同 worktree 再编 | S1 构建槽 + S2 同步 | 回到上次的槽，只重写变了的文件，Xcode 做增量 |
| G2 新 worktree 首编 | S1 构建槽 | 用这个项目唯一的槽，编过一次后就一直是热的 |
| G3 并行互不干扰 | S1 加锁；排队交给 aq | 一槽同时只有一个编译，其余等锁并输出在等谁 |
| G4 CI 快 | S1 + S3（和本机同一套） | 槽跨流水线保留，新分支也在热槽上增量 |
| G5 正确 | S4 依靠 Xcode + S2 | 同步保证内容变了的文件一定重写、没变的不动，其余交给 Xcode |
| G6 看得见 | S5 输出 | 汇总行、阶段耗时、P4/P5 核对、未命中最多的 target |
| G7 零配置 | S6 默认值 | 没有任何配置文件也能跑 |

## 关键决策

| # | 决策 | 结论 | 理由 |
| --- | --- | --- | --- |
| D1 | 槽数 | 每个项目固定一个槽，不设上限；本机和 CI 相同 | 本机 30 天同项目编译零重叠，多槽几乎用不上 |
| D2 | 排队与资源管控 | 交给 aq；agent-build 只用槽锁保证同一槽不被同时写 | 复用现有机制，不另造一套 |
| D3 | 产物 | 交给调用方的东西（产物、结果包、归档、日志）放在调用方 worktree 的 `.agent-build/output/`，只被这个 worktree 下次产出同类东西的运行替换；产物在槽锁内用 APFS 克隆从槽里复制过去 | 槽是同项目所有 worktree 共用的，留在槽里会被别的 worktree 的编译改写：build-for-testing 和 test-without-building 之间、编完到装上模拟器之间都会拿错二进制。克隆 Swiftfin 1.1G 产物 0.43 秒（实测）。比过：每个 worktree 一个槽（丢掉共享的增量编译）、按 worktree 改编译设置的输出目录（换 worktree 就要重新链接）、编完继续持锁到调用方用完（别人都得等）、放在 XDG 缓存里（要另做回收；放进 worktree 随 worktree 删） |
| D4 | 编译前步骤 | 不提供；编译需要的东西归工程自己准备（前提 P8） | 职责边界：工程怎么准备是接入方的事 |
| D5 | 命令形态 | 在原命令前加 `agb`（agent-build 的命令名；不用 `ab`，因为 macOS 自带 `/usr/sbin/ab`）：`agb xcodebuild …`、`agb cargo …`；只接受这两个，cargo 只接受编译类子命令和 `clean` | agent 只需加一个词；agb 只管编译，改源码、发布、查询的命令直接用原工具 |
| D6 | 项目名与位置 | agent-build，`~/Workspace/self/agent-build` | 直白，和 agent-verify 同一风格，不绑定语言 |
| D7 | 技术选型 | Rust，照搬 agent-quay 的工程骨架（core / sys / 二进制三层、clippy deny、just check），零第三方依赖 | 以后扩到 Rust 和 Linux CI；复用你现有的骨架 |
| D8 | 配置 | 全局只有 `cache_limit` 一项（XDG、YAML 平铺键值），不理解项目 | G7；配置不理解项目概念 |
| D9 | 被 git 忽略的编译输入（P8） | 项目在提交进仓库的 `.agent-build/config.yaml` 里声明，同步时与被跟踪的文件同样对待；没声明的在编译失败时点名 | 槽的正确性是"在槽里编和在 worktree 里编结果一样"，被忽略的输入是唯一的缺口；只有项目知道哪些被忽略的文件是输入，所以由项目声明（同 Docker 构建上下文、Bazel 声明输入）。比过：全复制被忽略文件（会带进 DerivedData、`node_modules`，又慢又编出过时结果）、在槽里跑生成脚本（D4，越界） |

## 对外接口

对外只有一个命令、一个全局配置项。

### 命令

```
agb xcodebuild <xcodebuild 参数...>
agb cargo <build|check|test|bench|run|doc|clippy|rustc|clean> [参数...]
```

cargo 的规则见 [Cargo](#cargoagb-cargo) 一节；下面说的是 xcodebuild。

| 用法 | 例子 |
| --- | --- |
| 本机编译 | `agb xcodebuild build -workspace App.xcworkspace -scheme App -destination 'generic/platform=iOS Simulator'` |
| 本机测试 | `agb xcodebuild test -scheme Modules-Package -destination 'id=…'` |
| 从零编译 | `agb xcodebuild clean build -scheme App`（clean 交给 xcodebuild） |

- 参数原样交给 xcodebuild；输出位置由工具接管：`-derivedDataPath`、`-archivePath`、`-resultBundlePath`、`-clonedSourcePackagesDirPath` 传了会被丢掉并提示。`-workspace`、`-project`、`-xctestrun` 的值是 worktree 里的绝对路径时（含经符号链接的写法），改指向槽里的同一份并提示，否则编的是 worktree 里的工程，同步白做。指向 worktree 的 `.agent-build/output/` 的 `-xctestrun`（例如上一次 build-for-testing 的产物）不改指，相对路径补成绝对路径：槽里没有这份。`-resolvePackageDependencies` 按信息命令处理。产物、测试结果、归档、日志放在 worktree 的 `.agent-build/output/`（D3），输出末尾给出各自的路径；要长期保留由调用方自己复制。`-exportArchive` 不编译，本期不支持。
- 终端输出：见 S5。
- 退出码：0 成功；编译失败原样返回 xcodebuild 的退出码；64 参数错误（与 xcodebuild 自己的参数错误相同，sysexits 的 EX_USAGE）；125 同步失败（沿用 GNU env、timeout 表示"包装器自己失败"的惯例）；被信号终止为 128 + 信号号。

### 全局配置（可以没有）

```yaml
# $XDG_CONFIG_HOME/agent-build/config.yaml，默认 ~/.config/agent-build/config.yaml
cache_limit: 20G    # 每个项目的编译缓存上限（二进制单位，1G = 1024³ 字节）
```

- 只有一个键，没有项目概念。
- 格式是 YAML 的最小子集：只认 `键: 值` 和 `#` 注释，其他写法报错退出；自写解析约 20 行，保住 G8 的零依赖。
- agent-build 只读不写这个文件。

### 项目文件（可以没有）

```yaml
# <仓库根目录>/.agent-build/config.yaml，由项目提交进仓库；从 worktree 读取
ignored_inputs:      # 编译需要、但被 git 忽略的路径（文件或目录），相对仓库根目录
  - rust-core/build/
  - App/Generated.swift
```

- 只有这一个键，只属于这个项目，全局配置仍然不理解项目。
- 列出的路径同步时与被跟踪的文件同样对待：内容、可执行位、符号链接不同才写；worktree 里没有的从槽里删掉；从列表里去掉的路径下次同步时从槽里删掉，编译不会用到过时的副本。
- 写错（绝对路径、`..`、未知键）退出码 64，指出行号，不编译。

## 存储结构

两处，按归属分：全局目录只放各项目共享的编译状态，按 [XDG Base Directory 规范](https://specifications.freedesktop.org/basedir-spec/latest/) 放；worktree 里只放这个项目的配置和交给这个 worktree 调用方的东西。标"中间"的只在一次运行里需要。

```
$XDG_CONFIG_HOME/agent-build/config.yaml   用户配置，可以没有；只读
$XDG_CACHE_HOME/agent-build/               全部可删，删了只会变慢
└── slots/
    └── <项目名>-<地址哈希前 8 位>/          每个项目一个槽（本机和 CI 相同），例如 my-app-1a2b3c4d/
        ├── lock                           flock 锁文件：agent-build 持有，并交给 xcodebuild 继承；最后一个持有进程退出时由内核释放
        ├── owner                          最近的持有者（pid、worktree），等待者据此输出在等谁；每次加锁时覆盖
        ├── ignored_inputs                 上次同步的被忽略路径列表，用来删掉不再声明的路径
        ├── source/                        git 克隆：槽里的代码；建到一半中断的，下次运行开头删掉重建
        ├── DerivedData/                   xcodebuild 靠它增量编译，含 SourcePackages；由 Xcode 自己管理；用 xcodebuild 编过才有
        ├── CompilationCache/              这个项目的编译缓存；编完超过 cache_limit 就整个删掉；用 xcodebuild 编过才有
        └── target/                        cargo 的 target 目录，靠它增量编译；不设上限（同 DerivedData）；用 cargo 编过才有
<worktree>/.agent-build/
├── config.yaml                            项目文件，可以没有；由项目提交
└── output/                                交给调用方的东西；随 worktree 删
    ├── .gitignore                         内容是 *：整个目录不进 git，也就不会被同步进槽
    ├── Products/                          产物：编出产物的运行在槽锁内从槽里克隆过来；被这个 worktree 下次编出产物的运行替换
    ├── result.xcresult                    结果包：每次运行（信息命令除外）开头删掉，xcodebuild 直接写到这里
    ├── <scheme>.xcarchive                 归档：archive 运行开头删掉所有旧的，xcodebuild 直接写到这里
    ├── target/                            cargo 的产物（本次要求的包的 .a、.dylib、可执行文件），路径与 target 目录里相同；被这个 worktree 下次 build、rustc 替换
    ├── build.log                          xcodebuild 或 cargo 的完整输出：每次运行（信息命令除外）重写
    └── .staging/                          中间：克隆产物时先放这里，成功后改名为 Products/；中断留下的，下次运行开头删掉
```

- 默认路径：`~/.config`、`~/.cache`。
- **槽目录名**：origin 地址最后一段加地址哈希前 8 位，同名的不同仓库（例如 fork）不会撞；没有 origin 时用主仓库 git 目录路径算哈希。不需要配置。
- **交给调用方的东西**在 worktree 的 output 里（D3）。每样只被这个 worktree 下次产出同类东西的运行替换：test-without-building 不动它要测的产物，信息命令什么都不动。别的 worktree 的编译碰不到它。
- **磁盘占用** = 各项目槽大小之和，每个槽里的编译缓存不超过 `cache_limit`（编完超限就清掉），随项目数增长；my-app 一个槽约 8GB（本次实验观察值）。不用的项目直接删它的槽目录。每个 worktree 另有一份产物：克隆刚做完时不占额外空间，之后槽里被改写的部分才由这份单独占着，最多一份产物大小。

## 一次运行的流程与各机制

build 和 test 走同一条流程，共 6 步；每步对应下面一个机制。

1. **解析参数**：参数全部交给 xcodebuild，只丢掉`输出位置参数（-derivedDataPath、-archivePath、-resultBundlePath）`。读 `config.yaml`，没有就用默认值（S6）。
2. **定位**：worktree 根目录、当前提交、未提交改动列表、仓库标识。
3. **认领槽**（S1）：用本项目的槽（没有就创建）并加锁；槽正被占用就等它释放，并输出在等哪个 worktree 的编译。
4. **同步**（S2）：把槽里的代码变成和 worktree 一样，只重写内容不同的文件。失败退出码 125。
5. **编译**（S3、S4）：运行 xcodebuild，加上槽的 DerivedData 和缓存设置；输出实时转发。
6. **收尾**（S5）：核对 P4、P5，写汇总，释放锁，返回 xcodebuild 的退出码。

中途收到 Ctrl-C 或终止信号：停掉 xcodebuild，释放锁。

### S1 构建槽（G1、G2、G3、G4）

- **选择**：每个项目一个固定路径的槽 = 独立 git 克隆 + 常驻 DerivedData。路径固定是增量编译跨 worktree 有效的根本原因（DerivedData 里记的是绝对路径）。
- **并发**：槽用 flock 锁，同一时刻一个编译；后到的等锁，并输出在等哪个 worktree。谁先跑、机器同时跑几个由 aq 决定。
- **比过的候选**：每个 worktree 一个槽（槽数随 worktree 增长）；固定多槽加选槽规则；弹性槽加整机上限和队列。本机 30 天同项目编译请求零重叠，多槽几乎用不上，排队 aq 已经会做；按优先级 2（克制、复用现有的）选一个槽。
- **CI**：和本机完全一样，每个项目一个槽，不加参数。runner 配成同一项目不并发即可；并发时在槽锁上排队，结果仍正确。
- **出错与恢复**：锁是 flock，文件描述符交给 xcodebuild 继承。agent-build 即使被 kill -9，残留的 xcodebuild 及它启动的工具仍持有锁，全部退出后内核才释放，所以不会出现两个编译同时写一个槽，也不需要判断失效锁或 pid 复用。
- **反例**：同项目 worktree A、B 轮流编译，每次切换多做 A、B 之间差异的工作，G1 不严格成立。结果仍正确，只是慢；这是"一个项目一个槽"的代价。

### S2 同步（G1、G5）

- **选择**：新槽从本机仓库克隆，不联网 → 缺提交时从调用方仓库 fetch → 强制切到该提交 → 清掉多余文件 → 未提交改动按内容、可执行位、符号链接比较，只写不同的文件 → 子模块按调用方 worktree 的状态递归镜像（没初始化的就不初始化，不联网）→ 项目声明的被忽略路径镜像进槽（D9）：大小、修改时间、权限都和槽里一致就跳过、不读内容，否则按内容比较，写入时带上 worktree 的修改时间。这是 rsync 默认的 quick check（[rsync 手册](https://download.samba.org/pub/rsync/rsync.1) `--checksum` 一节）。编译改写过的槽内文件修改时间会变，下次仍会比较、恢复。盲区：内容变了但大小和修改时间都没变（例如用 `touch -r` 刻意还原时间），这种改动会被漏掉，和 rsync 相同。
- **开销**：声明一个 10 万个文件、约 500MB 的目录，不改动时每次同步 0.5～0.8 秒；改之前每次都读两边内容比较，同机同目录要 25～30 秒（release 构建、文件已在系统缓存里，实测）。升级后第一次同步仍要全读一遍，并补上修改时间。
- **比过的候选**：每次全量拷贝（会改所有文件的修改时间，增量失效）；git worktree（受调用方 worktree 删除影响）。
- **出错与恢复**：同步失败（例如磁盘满）→ 退出码 125，不编译；下次同步先强制切换再清理，槽不会卡在半同步状态。

### S3 编译缓存（G1、G4）

- **选择**：每个项目一份编译缓存，放在它的槽里（`slots/<项目>/CompilationCache/`）；只打开 Xcode 自带的编译缓存，不做路径映射。
- **上限由 agb 执行**：Xcode 27.2 不执行 `COMPILATION_CACHE_LIMIT_SIZE`（实测：设 2G，Swiftfin 冷编后长到 3.1G），而缓存格式是 Xcode 的，外部只能整个删、不能只删旧的。所以每次编译结束、仍持有槽锁时，agb 量这个项目的缓存占的磁盘（按块，同 `du`），超过 `cache_limit` 就整个删掉并输出一行；这次编译已经用完缓存，只影响下一次。缓存是这个项目自己的、又在槽锁内，不会删到别的编译正在用的缓存。上限仍传给 Xcode，以后的版本执行时 agb 就很少需要动手。量大小的开销：6.8G、约 1.7 万个文件的缓存约 0.7 秒（机器高负载时实测）。
- **为什么不整机共用一份**：共用时删缓存会碰到别的项目正在跑的编译，要另加一把全机锁；共用的收益只是不同项目各自首次编译时少编一遍系统模块。每个项目一份还让"删掉槽目录就忘掉这个项目"连同缓存一起成立。代价：磁盘总量是项目数 × 上限。
- **作用**：同一项目始终在固定路径编译，切回编过的提交（如 A → B → A）时大多直接命中；槽被删后在同一路径重建，也能命中。
- **比过的候选**：打开前缀映射并补两条映射，不同路径之间的命中率从 41% 升到 96%（实测）。一个项目一个槽后路径不再变化，映射只在不同项目共享同一份源码时有用，却带来调试信息路径的未验证风险；按优先级 2 去掉。
- **什么情况重新考虑**：同一项目又需要在多个路径编译（例如恢复多槽）。

**注入的设置只有缓存必需的**：打开编译缓存、缓存目录、保留缓存目录、缓存上限、缓存诊断输出（用来数命中），再加槽的 `-derivedDataPath` 和 `-archivePath`。不改其他任何编译设置（例如不关代码索引、不关 dSYM）。

### S4 正确性依靠 Xcode（G5）

- **选择**：agent-build 只保证同步时内容变了的文件一定重写、没变的不动；是否复用完全交给 Xcode 的增量判断和缓存 key。
- **比过的候选**：出错后自动重编（兜底，不符合 G5）；自建内容清单（另造判断，违反优先级 1、2）。
- **依据**：4 个实验共 12 步，Xcode 判断全部正确（已运行）。
- **反例**：曾有一次无法复现的失败。如果再出现，完整日志保留在 worktree 的 output 里查根因，工具不掩盖。

### S5 输出与前提核对（G6）

- **选择**：编译期间 xcodebuild 的原始输出只进日志，终端只有一行起始提示（含日志路径）；编完用 `xcrun xcresulttool` 读 agb 注入的结果包（`-resultBundlePath <worktree>/.agent-build/output/result.xcresult`），按下表输出。只打印信息的命令（`-showBuildSettings`、`-list` 等）没有结果包，原样输出。
- **比过的候选**：从滚动的原始输出里按文字匹配取数（Apple 改输出格式就悄悄失效，同一个错误在原始输出里会重复多次）；原样转发全部输出（agent 要读几千行才找到错误）。结果包是 Xcode 给工具读的结构化结果，字段有类型。

| 输出 | 来源（都是 xcresulttool 的 JSON） | 开销 |
| --- | --- | --- |
| 错误，按 `路径:行:列: error: 信息` | `get build-results` 的 `errors`（位置从 `sourceURL` 取，行列从 0 数，加 1） | 样例工程 22ms（实测） |
| 没让编译失败的错误（退出码 0 时结果包里仍记着的错误，典型是构建脚本打印 `error:` 却以 0 退出，Xcode 另记一条 "emitted errors but did not return a nonzero exit code"）：一条一行 `agb: error that did not fail the build: …`，不按编译器格式、不计入 `errors=`，让错误数和退出码一致 | 同上；按退出码分：退出码 0 说明没有哪个错误让编译失败 | 无 |
| 失败的测试和原因（只在 test） | `get test-results summary` 的 `testFailures` | 同上量级 |
| 命中/未命中 | `get log --type build` 根节点的 `BuildOperationMetrics` 附件（Xcode 自己的计数） | 样例工程 52ms（实测），大工程未测 |
| 未命中最多的 target（P6 自查入口） | build log 里带缓存未命中提示的任务，按任务所属 target 汇总 | 同上 |
| 阶段耗时：解析依赖、依赖图、规划、执行 | build log 顶层各节的 `duration` | 同上 |
| P4 没满足的 target | `build-results` 里有"caching requires explicit module build"警告时，Swift 编译任务都没有缓存提示的 target | 同上 |
| P5 被改写的文件 | 编完在槽里 `git status` | 约 0.4 秒（实测） |
| P8 缺的被忽略文件（只在编译失败时） | 从错误里找出槽内路径，挑出 worktree 里有、槽里没有、被 git 忽略的 | 无 |
| 结果包、产物、归档路径（编译失败时只给结果包；没编译的 test-without-building 不给产物） | worktree 的 `.agent-build/output/` | 产物克隆 0.43 秒（Swiftfin 1.1G，实测） |
| 汇总行：结果、槽、同步耗时、编译耗时、命中/未命中、错误数、警告数、日志位置 | 以上各项 | 无 |

- 路径改写：输出里的槽路径改写成 worktree 路径，否则 agent 会去改槽里的文件。macOS 的 `/var`、`/tmp`、`/etc` 是指向 `/private/...` 的链接，Xcode 输出这类路径时会去掉 `/private`；所以 agb 算出槽路径时就统一成 Xcode 的写法（`/private/tmp/…` 记作 `/tmp/…`），编译和改写都用这一种写法。
- **出错与恢复**：编译失败但读不到结果包（工程文件读不了、xcodebuild 没起来）→ 输出原始输出的最后 30 行，不让失败无声。结果包和日志每次运行前删掉重建，只保留最近一次。
- **反例**：Xcode 改了结果包的字段名 → 读不到的项为空（错误列表空、计数为 0），编译结果和退出码不受影响；失败时有原始输出最后 30 行兜底。冒烟测试用真 xcresulttool 校验一次编译错误能读出来。
- 输出末尾示例：

```
/Users/me/my-app/App/Login.swift:42:9: error: Cannot find 'session' in scope
agb: phases resolve=7.7s graph=5.5s plan=2.0s execute=18.6s
agb: precondition P4 not met: App (explicit modules off) — their Swift compiles are not cached
agb: top misses: MXAIModule 15, MXAIUIModule 8
agb: results=/Users/me/my-app/.agent-build/output/result.xcresult
agb: failed slot=my-app-1a2b3c4d sync=1.1s build=35.9s cache hit=46 miss=23 errors=1 warnings=12 log=/Users/me/my-app/.agent-build/output/build.log
```

### S6 配置与默认值（G7）

- 没有 `config.yaml` 时：`cache_limit` = 20G，每个项目。
- 20G 的来源：Swiftfin 冷编一次缓存约 3.1G（实测），20G 约能装下 6 个分支的编译结果；是假设，按价值验证和实际使用的数据再调。
- 全局配置不按项目区分；项目只能在自己仓库的 `.agent-build/config.yaml` 里声明被忽略的编译输入（D9），编译前需要准备的东西仍归工程自己（D4）。

## Cargo（agb cargo）

和 xcodebuild 用同一套机制：同一个槽、同步、槽锁、`.agent-build/config.yaml`、worktree 的 output、输出路径改写。只说不同的地方。

**做法**：在槽里运行 cargo，`CARGO_TARGET_DIR` 固定为槽里的 `target/`，加 `--message-format=json` 读 Cargo 的结构化消息；清空 rustc wrapper；编完把本次产物拷进 worktree 的 `output/target/`。同一项目既用 xcodebuild 又用 cargo 时共用一个槽和一把锁，构建目录和产物各放各的。

| 子命令 | 处理 |
| --- | --- |
| `build`、`rustc` | 编译；成功时把本次要求的包的 `.a`、`.dylib`、可执行文件拷到 `output/target/`（路径同 target 目录），先删旧的；失败不给产物 |
| `run`、`bench` | 编译并运行；程序自己的输出原样显示 |
| `test` | 编译并测试；失败的测试按 `agb: test failed: 名字: 路径:行:列: 原因` 列出 |
| `check`、`clippy`、`doc` | 只看诊断；不动 output 里的产物 |
| `clean` | 清空槽里的 target 目录，cargo 的输出原样显示 |
| 其他（`fmt`、`fix`、`clippy --fix`、`add`、`update`、`install`、`publish`、`metadata`、`tree` 等） | 退出码 64，提示直接用 cargo：改的是槽里的文件，下次同步就被冲掉；或写到槽外、或不编译 |

- 参数：`--target-dir`、`--message-format` 和环境变量 `CARGO_TARGET_DIR` 由 agb 接管，传了丢掉并提示；`--manifest-path` 指向 worktree 时改指向槽里的同一份；`--` 之后原样交给程序或测试框架。
- 退出码：cargo 自己的（编译失败 101）；64、125、128 + 信号号同 xcodebuild。

**正确性（G5）**：Cargo 按源码文件的修改时间判断要不要重编，不看内容。两个 worktree 共用一个 target 目录时，旧提交的 worktree 文件更旧，Cargo 会直接用别的 worktree 编出的产物（实测 h2：1.3 秒、重编 0 个单元）。槽里只有一份源码，同步只重写内容变了的文件，重写的文件修改时间是新的，所以修改时间可信；A22 用旧提交、旧修改时间的 worktree 守住这一点。在同一个槽里 A→B→A 切换，切回后的 `libcore_ffi.a` 与冷编逐字节一致（rust-sdk，dev 与 release 各一组，实测）。

**不接 rustc wrapper（kache、sccache）**：wrapper 按自己的 key 复用产物，key 漏掉影响结果的输入就会编错：kache 的 rustc key 不含 `IPHONEOS_DEPLOYMENT_TARGET`，改了部署目标后产物里仍有 6 个 crate 是旧的 `minos 13.0`（2026-11-12 实测）。按优先级 1，复用只由 Cargo 自己判断：agb 把 `RUSTC_WRAPPER`、`CARGO_BUILD_RUSTC_WRAPPER`、`RUSTC_WORKSPACE_WRAPPER`、`CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER` 设为空（空值也盖过 Cargo 配置文件里的 `build.rustc-wrapper`），环境里设过的在输出里点名一行。代价：切回编过的提交时，内容变过的工作区 crate 要重编（rust-sdk 13 个，dev 约 8 秒，release 每个目标平台约 80 秒；有 kache 时约 1 秒，实测）。kache 还挡在构建脚本调用的 C 编译器前面（PATH 上的 `cc`、`cargo` shim，Cargo 配置 `[env]` 里的 `HOST_CC = "kache cc"`），上面几个变量管不到，agb 另设 `KACHE_DISABLED=1`：kache 的每一层都直接转给真编译器，不算 key、不读不写缓存（2026-11-21 实测：构建脚本经 `HOST_CC` 和 PATH 上的 `cc` 各编一次 C，直接用 cargo 时 kache 记了两次 `cc cache key`，经 agb 一次也没有）。这是 agb 里唯一认得 kache 的地方；其他 C 编译缓存（ccache、sccache 经 `CC` 接入）不处理。

**前提**（在 P3、P5、P8 之外）：

| # | 前提 | 不满足时 |
| --- | --- | --- |
| R1 | 编译结果只取决于源码、`Cargo.lock`、工具链和构建脚本声明过的输入（`rerun-if-changed`、`rerun-if-env-changed`）；本机和 CI 的环境变量不同由接入方负责 | 声明过的变了：Cargo 重跑，正确但慢；没声明的影响了产物：不保证，agb 不检测（同 P6） |
| R2 | 多步出包（生成绑定、lipo、打包）读 `output/target/` 里的产物，不读槽 | 读到别的 worktree 后来编的东西 |

**输出**（终端，编完）：错误按 `worktree 路径:行:列: error: 信息`（rustc 的完整文字在 `build.log`）、失败的测试、`agb: rebuilt: <重编的本项目包>`、P5、P8、`products=`、汇总行 `agb: ok slot=… sync=… build=… units fresh=N rebuilt=M errors=… warnings=… log=…`。`fresh`、`rebuilt` 来自 Cargo 消息里每个单元的 `fresh` 字段。构建脚本的运行次数不显示：Cargo 对没重跑的构建脚本也发 `build-script-executed`，数不出重跑了几个（实测：空编译仍有 26 条）。

**比过的候选**：让 kache 留在槽里（切回快，但继承它的 key 漏洞，违反 G5）；Rust 1.99 的共享 build-dir（跨 worktree 用错产物，见上）；`agb run` 在槽里跑整个出包脚本（越过"只管编译"，D4）；把整个 target 目录拷给调用方（GB 级中间产物）。

## 多项目与多 worktree（G3）

每个项目一个槽，同一项目的所有 worktree 共用它；不同项目互不影响；不设槽数上限。

- **和 aq 的分工**：aq 决定什么时候放行、机器同时跑几个；agent-build 只保证同一个槽不被两个编译同时写。不经过 aq 直接调用时，槽锁保证正确，但不控制机器负载。
- **CI**：和本机同一套规则、同一条命令，只需把 runner 配成同一项目不并发。

## 什么时候不成立

agent-build 只负责两件事：把 worktree 原样同步进固定路径的槽，并替 xcodebuild 打开编译缓存。其余都由别人负责，所以"不成立"只可能来自下面五类。只有第 1 类是我们的 bug，其余我们只保证不掩盖。

| # | 归谁 | 条件 | 表现 | agent-build 做什么 |
| --- | --- | --- | --- | --- |
| N1 | agent-build | 同步漏了变化：内容变了的文件没重写，或槽里有 worktree 没有的文件 | 结果和从零编译不一致（G5 失败） | 这是我们的 bug，用例守住：任一提交和未提交状态同步后，槽里的被跟踪和未忽略文件与 worktree 逐字节相同 |
| N2 | Xcode | 增量判断或缓存 key 出错 | 结果错或报了不该有的错 | 不重试、不清理；保留完整日志。用户可自己 `clean build` 或删槽 |
| N3 | Xcode | Xcode 升级改了缓存行为（P2） | 命中率变化 | 不保证；升级后重新验证 |
| N4 | 工程 | 依赖被 git 忽略的本地文件（P8） | 在槽里编不过 | 声明进 `.agent-build/config.yaml`（D9）；没声明时编译失败，输出点名缺的被忽略文件 |
| N5 | 工程 | 构建脚本写死了 worktree 的绝对路径，或往槽和 DerivedData 之外写东西 | 编译读写到错误位置，可能串到别的 worktree | 不检测；属于 P8（从干净检出、任意路径都能编）的一部分 |
| N6 | 工程 | 没开 explicit modules（P4）、改写被跟踪文件（P5）、宏插件不可复现（P6） | 正确但变慢；P5 还会让每次同步都把被改写的文件改回去，触发重编 | P4、P5 在输出里指出；P6 只列未命中最多的 target |
| N7 | 工程 | 子模块在调用方 worktree 里没初始化（P3） | 槽里同样没有，编不过 | 原样镜像 worktree，不替工程联网初始化 |
| N8 | 环境 | 首次编译时连不上依赖源 | SwiftPM 解析失败 | 原样输出 |
| N9 | 环境 | 磁盘不够（槽随项目数增长，缓存有上限） | 同步或编译写盘失败 | 不管理槽数；用户删不用的槽目录 |
| N10 | aq / CI 配置 | 绕过 aq 直接并发调用，或 CI runner 把同一项目配成并发 | 结果正确，但机器过载或在槽锁上长时间排队 | 只保证同一槽不被同时写 |
| N11 | 调用方 | 同项目两个 worktree 轮流编译 | 每次切换多做两者差异的工作，G1 不严格成立 | 这是一个项目一个槽的代价 |
| N12 | 调用方 | 编完后过一阵才去拿产物，期间同一个 worktree 又跑了一次 agb | 拿到的是这个 worktree 新一次的产物 | 产物在 worktree 的 output 里，别的 worktree 的编译碰不到；输出说明它被这个 worktree 下次编译替换，要保留就复制 |
| N13 | 调用方 | 传了输出位置参数并期望生效 | 参数被丢掉 | 提示已丢掉，并给出实际位置 |
| N14 | 工程 | worktree 里有未跟踪的嵌套仓库（例如切到旧提交后残留的子模块目录） | 槽里没有它 | 按 git 的规则视为另一个仓库，不同步 |

## 横切项

| 项 | 结论 |
| --- | --- |
| 数据归属 | worktree 是唯一可信来源；槽、编译缓存都可以删了重建，删掉只会变慢；`config.yaml` 归用户，agent-build 只读 |
| 并发与时序 | 槽用 flock 锁认领，一槽同时只有一个编译；锁随持有它的进程（agent-build 和它交出锁的 xcodebuild）全部退出才释放；agent-build 被终止时先停掉 xcodebuild，避免两个 xcodebuild 写同一槽。共用缓存的多进程读写由 Xcode 负责：并行测试中两个编译同时读写没有出问题（实测），缓存库本身是否保证多进程安全是凭记忆，未核实 |
| 持久化与迁移 | 共享的编译状态在 XDG 缓存目录，交给调用方的在 worktree 的 `.agent-build/output/`；不迁移旧布局（`.agent-build.yaml`、槽里的 `src`、`dd`、`cas`、`archives`），旧槽改名后冷编一次；升级 Xcode 后旧缓存条目自然失效，靠上限淘汰 |
| 可观测 | 每次编译的输出见 S5；完整日志在 worktree 的 `.agent-build/output/build.log`，只留最近一次 |
| 演进 | 预扫描缓存（swift-build 56b0187）正式发布后再评估；多端（Android 等）另起卡片，不在本期抽象 |

## 验收用例

用例和每条的状态维护在 [cases.md](cases.md)：外层验收 A1–A10，内层细节 A1.1 等。`just check` 核对用例表与测试一致，`just smoke` 用真 xcodebuild 跑 A1。

## 任务

**进度**：T1–T6 已完成；T7 未做，等 my-app 满足 P8。

在 `~/Workspace/self/agent-build` 用 Rust 重写，工程骨架照搬 agent-quay。每项都以"列出的验收用例通过 + `just check` 全绿"算过。

```
T1 工程骨架 ──┬── T2 core 判断逻辑 ──┐
               └── T3 样例工程与夹具 ─┤
                                     ├── T4 sys 外部操作 ── T5 编排与输出 ── T6 文档
                                     │
                          T7 实测（在 T5 之后，且接入方工程满足 P8）
```

| # | 任务 | 内容 | 做到什么算过 | 依赖 |
| --- | --- | --- | --- | --- |
| T1 | 工程骨架 | Cargo workspace：`agent-build-core`（纯逻辑）/ `agent-build-sys`（外部操作）/ 二进制；clippy 禁止 unwrap/expect/panic 和超长函数；`just check`（格式、clippy、测试、单文件 ≤ 400 行、无第三方依赖、用例表核对）；`docs/cases.md`。移除旧 Swift 实现前先列清单给用户确认 | U80、U81、U82 在空实现上跑通 | — |
| T2 | core 判断逻辑 | 参数解析（第一个词、输出位置参数、`-exportArchive`）、配置解析（YAML 子集）、槽目录命名、未提交改动解析、失效锁判断、日志解析（命中数、按 target 汇总、P4 警告、阶段切分）、路径改写、缓存提示过滤、汇总行格式、注入的编译设置清单 | U2、U3、U5、U8、U19、U35、U43、U44、U54、U55、U56；U4、U40、U50、U60 的单元部分 | T1 |
| T3 | 样例工程与夹具 | 一个小的 Xcode 工程：两个 Swift 模块、一个被引用的 ObjC 公开头、一个关掉 explicit modules 的 target、一个改写被跟踪文件的脚本阶段、一个子模块；构造临时 git 仓库和各种改动状态的测试工具 | 样例工程用裸 xcodebuild 能从干净检出编过；夹具能构造 U14 列的全部状态 | T1 |
| T4 | sys 外部操作 | git（本地克隆、fetch、强制切换、清理、status）、按内容比较复制未提交改动、子模块镜像、槽锁（flock，交给 xcodebuild 继承）、运行 xcodebuild 并流式输出、信号转发、XDG 路径、日志轮转 | U14、U17、U20、U21、U22、U23、U33、U34、U66 | T2、T3 |
| T5 | 编排与输出 | 把 6 步流程串起来；退出码；等锁提示；汇总、阶段耗时、P4/P5、未命中最多的 target；产物、.xcresult、.xcarchive 路径 | U1、U4、U6、U7、U10–U13、U15、U16、U18、U30–U32、U36、U40、U41、U45、U50–U53、U57–U60、U67 的集成部分 | T4 |
| T6 | 文档 | `README.md` / `README.zh-CN.md`：用法、前提 P1–P8、什么时候不成立、P6 自查方法（只删宏插件再编，看命中率是否下降）、产物路径有效期；`docs/design.md` 替换为本方案 | 文档里的每条行为都能对上一条验收用例；不再出现已删掉的设计 | T5 |
| T7 | 实测 | 在 my-app 上跑 U13、U15、U16、U41、U65，记录耗时和命中数 | 结果记录在案；U15 的逐字节对比通过 | T5；my-app 先满足 P8（它的 rust-core 产物现在靠编译前脚本生成，属于接入方的事，不在本方案内） |

**不在本方案内的后续**：CI 如何安装 agent-build、runner 配置；接入方工程满足 P4、P5、P8；清理旧版留下的缓存目录。

## 相对旧原型的删减

旧 Swift 原型里的下列内容在本方案中删掉，删后没有目标受损。

| 删掉的 | 理由 | 去处 |
| --- | --- | --- |
| 出错后自动重编、旧产物错误特征列表 | 兜底，不符合 G5 | S4 |
| 仓库里的 `.fastbuild.json` | G7；槽数属于机器不属于项目 | 全部去掉：槽数固定为每项目一个，编译前要准备的东西归工程自己（P8） |
| 条件设置（Xcode 27 时提部署版本） | 属于修工程 | 接入方 |
| 检查步骤 | 属于约束检查，已移出范围 | 接入方 |
| `--pool`、`--slots`、`--dedicated` | 一个项目一个槽，CI 和本机相同，不需要任何选项 | S1 |
| `--key` 和 key 概念 | 一个项目一个槽，不需要选槽 | S1 |
| `--clean`、`--no-retry`、`--no-export`、`--print-dd` | clean 交给 xcodebuild；重编已删；不导出；产物路径在输出里 | — |
| `FASTBUILD_CACHE_DIR` | 统一用 XDG 环境变量 | 存储结构 |
| `agent-build report` | G6 只要求每次可见，不要求趋势 | `以后需要时再加` |
| 宏插件指纹检测（P6） | 要自己拼出插件的全部输入，少算一样就误报，等于另造输入追踪 | 未命中最多的 target + 文档自查方法 |
| 内容清单 | 另造判断，Xcode 已经负责 | S4 |
| 多槽、选槽规则、弹性扩槽、槽数上限与队列 | 30 天同项目零重叠，多槽几乎用不上；排队和资源管控 aq 已经会做 | 一个项目一个槽 + 槽锁；排队交给 aq |
| 新槽复制依赖检出（SourcePackages） | 一个项目一个槽后只在首次编译触发；能否联网拉依赖是环境的事，直接跑 xcodebuild 也一样 | 首次编译由 SwiftPM 自己下载（my-app 约 23 个依赖、2.2GB） |
| stats.jsonl 统计记录 | 本期没有功能读它；每次编译的输出和完整日志已满足 G6 | — |
| 替调用方联网初始化子模块 | 属于替工程做准备；槽只镜像 worktree | S2 |
| 关闭代码索引（COMPILER\_INDEX\_STORE\_ENABLE=NO） | 不是缓存需要的设置，改变了工程的编译产出 | S3：只注入缓存必需的设置 |

**检查过、决定保留的**：输出路径改写、中断处理、P4/P5 核对和阶段耗时，理由分别写在 S2、S5 和横切项里。

## 证据与数据来源

证据等级：已运行 > 原型实验 > 引用 > 推演；标"推演"和"假设"的就是剩下的风险。实验数据未随仓库发布。

| 结论 | 数字 | 等级 | 来源 |
| --- | --- | --- | --- |
| 热槽让新 worktree 首编变快 | 178 秒 → 45 秒 | 已运行（前期原型；当前版本待 A10 复测） | 基准测试，my-app，约 2400 个 Swift 文件，Xcode 27.2 |
| 宏插件重编导致未命中 | 23 个编译未命中，命中率 66.7% | 已运行 | s7-macro-rebuilt；[Swift 论坛同类问题](https://forums.swift.org/t/compile-cache-key-instability-from-non-deterministic-macro-plugin-binaries/86695) |
| Xcode 增量判断正确 | 4 个实验共 12 步全对 | 已运行 | g5 系列实验，`g5/*.txt` |
| 本机同项目编译请求不重叠（支撑一个项目一个槽） | 30 天 122 次、零重叠；每天中位 2 个 worktree、最多 7 个 | 已运行 | Claude 会话记录统计（2026-10-20 至 11-19）；不含后台编译、手动终端编译和 CI |
| 跨仓库编译有重叠 | 108 次中 12 次 | 已运行 | aq 数据库，最近 3 天 |
| P5 核对开销 | 约 0.4 秒 | 已运行 | 槽里 `git status` |
| 增量再编的时间花在哪 | 改一个文件约 45 秒；编译器处理改动约 10 秒 CPU，规划约 23 秒 | 已运行 | 分支会话的第二轮测量 |
| 缓存库多进程安全 | — | 凭记忆，未核实 | 查 llvm CAS 文档或源码 |
| 默认 cache\_limit: 20G（每个项目） | Swiftfin 冷编一次缓存 3.1G | 假设 | 接入后看磁盘占用和缓存命中 |
| Xcode 不执行缓存上限 | 设 2G，Swiftfin 冷编后 3.14G、再编 3.16G | 已运行（Xcode 27.2） | Xcode 升级后重跑 A16 |
