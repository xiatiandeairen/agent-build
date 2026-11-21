# 用例

每行守一个对外行为。外层 A1、A2 是用户能观察到的验收；内层 A1.1 等是支撑它的细节，放在能抓住问题的最便宜那一层。守它的测试在 `#[test]` 上方的注释里写编号（如 `// A3, A3.1`），搜编号就能找到；tests/ 里不守用例的测试写 `// outside the table: <原因>`。`scripts/check-cases.py` 核对两边对得上。

- 验证：单元（core 纯逻辑，不碰 git 和 Xcode，毫秒级）、集成（临时 git 仓库 + 假 xcodebuild，秒级）、冒烟（真 xcodebuild 编样例工程，`just smoke`，交付前必跑）、实测（my-app 上人工跑并记录，Xcode 升级后重跑）。
- 测试注入：集成测试用环境变量 `AGENT_BUILD_XCODEBUILD` 指向 `tests/fixtures/fake-xcodebuild`，它按 `FAKE_*` 变量输出录制的日志、按指定退出码结束、挂起或改写文件；HOME 和 XDG 目录指向临时目录。
- 状态：待实现的只计数；标为已实现的必须有标了编号的测试。全部为已实现时完成。

## 外层验收

| 编号 | 情形 → 预期 | 防什么错 | 验证 | 状态 |
| --- | --- | --- | --- | --- |
| A1 | 样例工程不写任何配置，运行 `agent-build xcodebuild build -scheme App -destination 'generic/platform=iOS Simulator'` → 退出码 0，末尾有汇总行，给出的产物目录里有 .app；再跑 `test` → 给出的结果包路径存在；再加一处编译错误 → 退出码 65，错误按 `worktree 路径:行:列: error:` 输出（真 xcresulttool） | 参数拼错、槽的 DerivedData 没生效、产物路径算错：只有真 xcodebuild 能暴露 | 冒烟 | 已实现 |
| A2 | 同一 worktree 不改再编 → 槽里 0 个文件的修改时间变化；改 1 个文件 → 只有它变；同项目新 worktree → 用同一个槽，只有两者差异的文件变 | 同步碰到没变的文件，Xcode 大面积重编（结果仍正确，最难发现） | 集成 | 已实现 |
| A3 | 同步后，槽里被跟踪和未忽略的文件与 worktree 逐字节相同，worktree 没有的槽里也没有，被忽略的不复制。逐一覆盖：切提交、修改、新增未跟踪、删除、暂存后删除、重命名、文件名含空格和中文、符号链接、可执行位变化、子模块换提交、嵌套子模块、上一轮复制进来的未提交文件这一轮已撤销 | 唯一属于我们的正确性 bug：编出来的东西和 worktree 不一致 | 集成 | 已实现 |
| A4 | 编译前后调用方 worktree 里 `.agents/build/output/` 以外的文件、index、HEAD、git 配置不变，`git status --porcelain --untracked-files=all` 前后相同；origin 指向不可达地址时照常完成；调用方没初始化的子模块在槽里也没有 | 改到用户仓库、偷偷联网、替工程初始化：这些错不会让编译失败 | 集成 | 已实现 |
| A5 | 同项目两个调用同时开始 → 后者打印"在等 <worktree>"，前者结束后才开始，两个都成功；不同项目不互相等；持有者收到 SIGTERM → xcodebuild 先停、再释放锁；持有者被 kill -9 → 后来者等残留的 xcodebuild 结束后再接管（锁是 flock，文件描述符交给 xcodebuild 继承，内核在最后一个持有进程退出时释放，不需要判断失效锁） | 两个编译同时写一个槽，结果被破坏且难以复现 | 集成 | 已实现 |
| A6 | 假 xcodebuild 打印报错并以 65 退出 → 报错原样输出（路径已改写），退出码 65，只调用一次；槽目录不可写 → 退出码 125，不调用 xcodebuild；完整日志在 worktree 的 `.agents/build/output/build.log`，汇总行的 `log=` 指向它 | 掩盖失败、偷偷重试、退出码让 CI 误判 | 集成 | 已实现 |
| A7 | 假 xcodebuild 配合用真 Xcode 录制的结果包 JSON（`tests/fixtures/xcresult`，由假 xcresulttool 提供）→ 编译期间终端只有一行起始提示，原始输出只进日志；编完依次输出错误（`路径:行:列: error: …`，路径已改写）、失败的测试、阶段耗时、P4、未命中最多的 target、P5、结果包和产物路径、汇总行（含 errors、warnings）；失败但读不到结果包时，输出原始输出的最后 30 行；下次同步把被改写的文件改回 | G6 的输出整体对不上：缺行、顺序乱、刷屏；失败时看不到原因 | 集成 | 已实现 |
| A8 | 没有配置文件 → 每个项目的缓存上限 20G，缓存目录是这个项目槽里的 `CompilationCache/`，DerivedData 是槽里的 `DerivedData/`；写 `cache_limit: 10G` → 传给 xcodebuild 的是 10G；配置写错 → 退出码 64，指出行号 | 配置没生效或写错时被悄悄忽略 | 集成 | 已实现 |
| A9 | `just check` 一条命令跑完格式、clippy（禁止 unwrap/expect/panic，函数 ≤ 60 行）、全部单元和集成、单文件 ≤ 400 行、无第三方依赖、用例表核对 | 代码质量约束悄悄退化 | 实测 | 已实现 |
| A11 | 工程依赖被 git 忽略的文件（P8）：没声明时编译失败、错误里带着这个文件的路径 → 输出点名这个被忽略的文件和声明方法（错误里只有符号名、没有路径时无从点名），被跟踪的文件报错不点名；在 `.agents/build/config.yaml` 声明后 → 文件、目录、可执行位、符号链接原样进槽，只重写变化的文件（大小和修改时间都没变的不读内容），内容变了但大小不变、编译改写了槽里的文件、只改了可执行位，下次都跟上 worktree，worktree 里删掉的槽里也删；从声明里去掉 → 槽里删掉；声明写错 → 退出码 64 带行号，不编译 | 编出来的东西用了过时的被忽略文件；失败时报错指向 worktree 里明明存在的文件，让人找错方向 | 集成 | 已实现 |
| A12 | 样例 SampleWorkspace 放在仓库的 `ios/` 子目录（`-workspace`，App 依赖一个本地 SwiftPM 包，包里有 Swift 和 ObjC 两个 target，ObjC 公开头被 Swift 引用），整个仓库放在路径含空格和中文的目录下；在 `ios/` 里 `agb xcodebuild build -workspace SampleWorkspace.xcworkspace -scheme App -destination 'generic/platform=iOS Simulator'` → 退出码 0，产物目录在仓库根的 `.agents/build/output/Products`，里面有 App.app；不改再编（output 已在 worktree 里）→ `miss=0`；ObjC 公开头加一个方法声明再编 → 退出码 0；造一个编译错误 → 错误行指向 worktree 里含空格和中文的路径；`agb xcodebuild -resolvePackageDependencies -workspace SampleWorkspace.xcworkspace -scheme App` → 退出码 0，没有 `products=` 行 | 真 xcodebuild 只跑过 `-project` 纯 Swift；公开样本里 workspace 50%、子目录 34%、ObjC 混编 39%、本地包 28%；worktree 里的 `.agents/build/` 被 Xcode 或 SwiftPM 当成输入，每次都重编 | 冒烟 | 已实现 |
| A13 | 在 SampleWorkspace 的包目录里 `agb xcodebuild test -scheme <包>-Package -destination 'id=<iOS 模拟器>' -only-testing:<一个改成失败的测试>` → 退出码 65，输出 `agb: test failed: <该测试>`；改回后退出码 0；回 `ios/` 不改代码再编 App → `miss=0`（切换容器会让部分任务重做，但全部命中编译缓存，不重新编译） | 在包目录里只给 `-scheme` 的 CI 占 18%；workspace 和包两种容器轮流用同一个 DerivedData，互相作废时每次切换都重编，G1 悄悄失效 | 冒烟 | 已实现 |
| A14 | SampleApp，同一仓库的两个 worktree A、B：A 让一个测试失败后 `agb xcodebuild build-for-testing -project SampleApp.xcodeproj -scheme CoreTests -destination platform=macOS` → 产物目录（A 的 `.agents/build/output/Products`）里有 `.xctestrun`；B 不改代码同样 build-for-testing；A `agb xcodebuild test-without-building -xctestrun <A 的那份> -destination platform=macOS` → 退出码 65，输出 `agb: test failed: …` 点名 A 改坏的那个测试；B 用自己那份 → 退出码 0 | 14% / 11% 的 CI 用两段式；两段之间同项目另一个 worktree 编过，测到的就是别人的二进制，结论错了还看不出来 | 冒烟 | 已实现 |
| A15 | 4 个公开 app 声明好被忽略的输入后（声明只在本地，不提交上游）：Swiftfin 模拟器 build；NetNewsWire 声明构建阶段生成的 `SecretKey.swift`、跑 macOS 测试计划；Bark 先 `pod install`、声明 `Pods/`、用 workspace build；GitUp（子模块）在 `GitUpKit` 子目录跑 macOS 测试 → 每个退出码 0；编完槽里 `git status` 干净，或 P5 行点名被改写的文件；不声明直接编 Bark → 失败并点名 `Pods/` 下缺的文件（错误里带路径）；记录每个 app 的 sync 耗时和读结果包耗时 | 远程 SwiftPM + 宏、CocoaPods、构建阶段生成的文件、子模块 + 纯 ObjC + macOS，样例工程证明不了；P8 点名在真实报错里能否找对文件；大工程上同步和读结果包的耗时 | 实测 | 待实现 |
| A16 | 用临时 XDG 目录写 `cache_limit: 2G`，在 Swiftfin 上冷编一次 → 编完缓存超过 2G 时，输出 `cache … exceeded cache_limit 2G and was cleared`，这个项目的缓存目录被删掉；再编一次 → 退出码 0 | Xcode 27.2 不执行 `COMPILATION_CACHE_LIMIT_SIZE`（实测设 2G 长到 3.1G）；agb 不清理时磁盘只增不减，后果难撤回 | 实测 | 待实现 |
| A17 | SampleApp：`agb xcodebuild archive -project SampleApp.xcodeproj -scheme App -destination 'generic/platform=iOS' CODE_SIGNING_ALLOWED=NO` → 退出码 0，输出的 `archive=` 路径是 worktree 的 `.agents/build/output/App.xcarchive`，里面有 `Products/Applications/App.app` | 21% 的 CI 直接 archive；注入的 `-archivePath` 和 archive 路径只在单元层测过 | 冒烟 | 已实现 |
| A18 | 调用方是 `--depth 1` 浅克隆 → 建槽成功，同步后逐字节相同；调用方再 `git fetch --depth 1` 到更新的提交 → 再次同步成功；删掉调用方，同一 origin 的新浅克隆（更新的提交）→ 用同一个槽同步成功，退出码 0 | 67% 的 CI 检出是浅克隆；槽从调用方本地 fetch，浅仓库之间互相 fetch 可能缺对象或被拒；出错时持久 runner 上每次 CI 都以 125 失败，G4 落空 | 集成 | 已实现 |
| A19 | 同一项目的两个 worktree A、B（假 xcodebuild 把是谁编的写进产物和日志）：A 编完 → 产物、结果包、日志都在 A 的 `.agents/build/output/`，输出行指向那里；B 再编 → 只有一个槽，A 的 output 里每个文件的内容和修改时间都不变；A 再编但失败 → A 的 output 里只有这次的结果包和日志，没有上次的产物；全程 B 的 output 不变 | 调用方装、测、导出用的是别的 worktree 的产物；output 里新旧两次的东西混在一起 | 集成 | 已实现 |
| A20 | 编完后在 worktree 里 `git add -A` → 暂存的只有项目自己的 `.agents/build/config.yaml` 和别的工具放在 `.agents/` 下的文件（如 `.agents/skills/`，内容不变），output 下的文件一个都没有；`output/.gitignore` 第一行写明由 agent-build 创建；下一次同步后槽的 `source/` 里有 config.yaml，没有 `.agents/build/output` | 产物被误提交；上一次的产物被当成源码；agb 动了 `.agents/` 里别的工具的文件改动同步进槽，每次多拷一份 | 集成 | 已实现 |
| A21 | 样例 CargoSample（两个 crate、构建脚本、静态库、程序，零依赖）：`agb cargo build` → 退出码 0，产物在 worktree 的 `.agents/build/output/target/debug/`，worktree 里没有 `target/`；不改再编 → `units fresh=4 rebuilt=0`；`agb cargo run --bin sample-tool` → 程序输出原样显示；测试失败 → 退出码 101，输出 `agb: test failed: 名字: worktree 路径:行:列: 原因`；编译错误 → 101，错误按 `worktree 路径:行:列: error: 信息` 输出，rustc 的完整文字在 build.log | 参数拼错、target 没固定在槽里、路径没改写回 worktree、程序输出被吞 | 集成 | 已实现 |
| A22 | 两个 worktree：X 在新提交（版本 v2），Y 在旧提交（v1）且源码修改时间早于任何一次编译；按 X、Y、X、Y 轮流 `agb cargo run` → 每次输出自己提交的版本 | Cargo 只看修改时间：共用 target 时旧 worktree 静默拿到别人的产物（实测 h2：1.3 秒、重编 0 个） | 集成 | 已实现 |
| A23 | X 编过后，同一提交的 worktree Y 编 → `rebuilt=0`；Y 只改了 sample-ffi 的程序 → `agb: rebuilt: sample-ffi` | 同步碰了没变的文件，换 worktree 就全量重编（结果仍正确，最难发现） | 集成 | 已实现 |
| A24 | 环境里 `RUSTC_WRAPPER` 指向一个一运行就失败的脚本 → 编译照常成功，输出 `agb: RUSTC_WRAPPER cleared` | kache 一类 wrapper 混进槽，带进它缓存 key 的漏洞（C 编译器前的缓存是前提 R3，不由 agb 处理） | 集成 | 已实现 |
| A25 | 带 `--target-dir`、环境变量 `CARGO_TARGET_DIR`、指向 worktree 的 `--manifest-path` → 前者丢掉并提示、目录不被创建，后者改指向槽并提示；`fmt`、`add`、`update`、`clippy --fix`、`install` → 退出码 64；`clean` → 清空槽里的 target 目录 | 改的是槽里的文件、下次同步就被冲掉；编的是 worktree 而不是槽，同步白做 | 集成 | 已实现 |
| A26 | build 后 output 的 `target/` 里只有 `debug/libsample_ffi.a` 和 `debug/sample-tool`；之后 `check`、`test` 不动它；编译失败 → 没有 `target/`，也不输出 `products=` | 拷了 GB 级中间产物；把旧产物当成这次的 | 集成 | 已实现 |
| A27 | 同一仓库：假 xcodebuild 编译进行中时 `agb cargo build` → 输出在等它、它结束后才编；两者都成功，output 里 `Products/` 和 `target/` 并存 | 两个工具同时写一个槽，或互相删掉对方的产物 | 集成 | 已实现 |
| A28 | rust-sdk 实测：两个 worktree 日常编译、A→B→A、一个 worktree 改代码后另一个切回、出包脚本那条 4 目标平台的 cargo build 改走 agb；记录耗时；切回后的 `libcore_ffi.a` 与冷编逐字节一致。2026-11-21 实测（44b1806，全新槽，load average 约 18）：dev 冷编 29.1 秒；不改再编 0.7 秒；同提交的另一个 worktree 0.7 秒、`rebuilt=0`；换到 Cargo.lock 不同的旧提交 B 20.2 秒、重编 17 个；切回 A 6.7 秒、重编 13 个，哈希与冷编相同；A2 加一个导出函数后编 3.5 秒、重编 1 个、产物里有该符号，A 再编 3.2 秒、哈希与冷编相同且没有该符号；出包 4 个目标平台冷编 218.7 秒，同提交另一个 worktree 1.1 秒、`rebuilt=0`、4 个 `.a` 哈希都相同（这组用 44b1806，当时 agb 还设 `KACHE_DISABLED=1`，之后按 R3 去掉）。同时编译（d29093e，热槽）：A 与 A2、A2 改代码与 A、A/A2/B 三个、A 出包与 A2 日常编译四组同时发起，持槽区间互不重叠，后到的输出在等谁，各自产物哈希正确（A2 的改动只出现在 A2 的产物里）；B 用过槽后 A 出包重编 52 个单元（13 个工作区包 × 4 个目标平台，122.7 秒），是槽内容来回切换的代价。未做：出包脚本其余步骤（UniFFI 生成、strip、xcframework）改读 `output/target/`，属接入方 | 样例工程证明不了真实项目上的收益和正确性 | 实测 | 已实现 |
| A10 | 价值验证：在公开 app 上按下方"价值验证"表跑 V1–V7（同机、交替跑基准和 agb、各 3 次取中位数、开跑前负载低于 CPU 核数），每项达到它的通过线；切回已编过的提交时产物可复现部分与冷编逐字节一致 | 速度声明没有基准、判不了对错；单次测量在负载噪声下不可比 | 实测 | 待实现 |

## 内层细节

| 编号 | 情形 → 预期 | 防什么错 | 验证 | 状态 |
| --- | --- | --- | --- | --- |
| A1.1 | 第一个词既不是 `xcodebuild` 也不是 `cargo`（`xcrun xcodebuild`、绝对路径、`swift`）、带 `-exportArchive`、没有参数 → 退出码 64 并提示正确写法或用法 | 把别的命令当编译执行 | 单元 | 已实现 |
| A1.2 | 带 `-derivedDataPath`、`-archivePath`、`-resultBundlePath` → 每个丢掉并各提示一行，其余参数顺序不变 | 丢参数时多吃或少吃一个值，参数错位 | 单元 | 已实现 |
| A1.3 | 注入的参数恰好是缓存设置 + 槽里的 `-derivedDataPath`，加上 worktree 的 `.agents/build/output/` 里的 `-resultBundlePath`，archive 时再加那里的 `-archivePath`；不含其他编译设置 | 越界改设置；漏注入 `-derivedDataPath` 而写进 Xcode 默认目录 | 单元 | 已实现 |
| A1.4 | 在不是 git 仓库的目录运行 → 退出码 64，说明需要 git 仓库 | 在任意目录乱建槽 | 集成 | 已实现 |
| A1.5 | 在 worktree 子目录里运行 → xcodebuild 的工作目录是槽里同一子目录；输出路径改写成 worktree 子目录下的路径 | 34% 的工程在子目录，这条路径零测试；算错时 xcodebuild 在槽根目录运行，找不到工程 | 集成 | 已实现 |
| A1.6 | 带 `-resolvePackageDependencies` → 按信息命令处理：注入槽的 `-derivedDataPath`（包解析进槽），不注入结果包，原样输出，不给产物路径 | 11% 的 CI 用它；落进编译分支会打印不存在的产物目录，并去读不存在的结果包 | 单元 | 已实现 |
| A1.7 | `-workspace`、`-project`、`-xctestrun` 的值是 worktree 里的绝对路径（含经符号链接的写法）→ 改写成槽里的同一路径，各提示一行；worktree 之外的路径不动；指向 worktree 的 `.agents/build/output/` 的（绝对或相对）→ 给 xcodebuild 这个 worktree 里的绝对路径，不提示（槽里没有这份） | 7% 的 CI 用 `$GITHUB_WORKSPACE/…` 指工程；原样传过去编的是 worktree 里的工程，同步白做，G1/G2 悄悄失效；output 里的 `.xctestrun` 被改指到槽里不存在的文件 | 单元；集成 | 已实现 |
| A1.8 | 带 `-clonedSourcePackagesDirPath` → 和其他输出位置参数一样丢掉并提示一行，包检出留在槽里 | 指向槽外时包在每次的新路径上编，缓存不命中 | 单元 | 已实现 |
| A2.1 | 槽目录名 = origin 最后一段 + 地址哈希 8 位；fork 同名不撞；没有 origin 时用主仓库 git 目录算；同一仓库的各 worktree 得到同一个名字 | 不同项目共用一个槽（结果错），或同一项目分到两个槽（变冷） | 单元 | 已实现 |
| A3.1 | 解析 `git status --porcelain -z`：首条以空格开头、文件名含空格、空输出 | 已踩过的"路径丢首字符" | 单元 | 已实现 |
| A5.2 | 不同项目的锁路径不同，同项目的相同 | 误用全局锁，不同项目互相等 | 单元 | 已实现 |
| A6.1 | 退出码映射：参数错误 64（同 xcodebuild）、同步失败 125（同 env、timeout 的包装器自身失败）、编译失败原样传递、被信号终止时为 128 + 信号号 | CI 按退出码误判 | 单元 | 已实现 |
| A7.1 | 汇总行字段：结果、槽、同步耗时、编译耗时、命中/未命中、日志位置 | 字段缺失或格式变化，依赖它的 agent 读不到 | 单元 | 已实现 |
| A7.2 | 从录制的结果包 JSON 读出：错误和警告及其位置（行列从 0 转成从 1 数，路径百分号解码）、失败的测试和原因、Xcode `BuildOperationMetrics` 里的命中/未命中计数、按 target 汇总的未命中任务 | 位置差一行、把非缓存计数算进去、按 target 归错 | 单元 | 已实现 |
| A7.3 | 从录制的 build log 的各节耗时切出阶段：解析依赖、依赖图、规划、执行，加起来等于总耗时 | 节名识别错，耗时归错阶段 | 单元 | 已实现 |
| A7.4 | Xcode 警告缓存需要 explicit modules 时，找出 Swift 编译任务都没有缓存提示的 target；没有警告时不报 | 漏报 P4，或在缓存正常时误报 | 单元 | 已实现 |
| A7.5 | 路径改写只在路径边界：槽路径 → worktree 路径；前缀相似的其他路径、DerivedData 路径不动；worktree 路径含空格；槽路径一开始就按 Xcode 的写法记（`/private/var/…` 记作 `/var/…`） | agent 去改槽里的文件，或路径被改坏 | 单元；集成 | 已实现 |
| A7.6 | 只打印信息的命令（`-showBuildSettings`、`-list` 等）原样输出 xcodebuild 的内容，不注入结果包；错误按编译器格式打印 | 查询命令被静默，或被注入它不认识的参数 | 单元；集成 | 已实现 |
| A7.7 | 按动作给出路径，都在 worktree 的 `.agents/build/output/` 下：有结果包就给结果包；build、test 成功且这次编出了产物 → 产物目录，说明"被这个 worktree 下次 agb 编译替换"；archive 成功 → .xcarchive，不给产物；没编译的（test-without-building）不给产物；编译失败只给结果包；信息命令不给 | 给出不存在、不对或会被别人改写的路径 | 单元 | 已实现 |
| A19.1 | 只有产出同类东西的运行才替换它：在子目录运行 → output 仍在 worktree 根；上次中断留下的 `.staging/` 在这次开头删掉；信息命令（`-showBuildSettings`）不动 output；test-without-building 用 output 里的 `.xctestrun` 跑 → 产物不变，结果包和日志换成这次的；archive → 删掉旧的 .xcarchive、留下这次的，产物不变 | 两段式测试的第二段删掉了它要测的产物；查一下设置就丢了上次编译的结果；半份产物被当成完整的 | 集成 | 已实现 |
| A19.2 | Swiftfin 模拟器 build：把 1.1G 产物克隆进 output 多花的时间 ≤ 1 秒（假设，依据单独测克隆 0.43 秒）；worktree 在另一个卷（`hdiutil` 建的磁盘映像）上 → 退回普通复制，产物完整，退出码 0。2026-11-21 实测：克隆 2440 个文件 0.44～0.47 秒，空编译时编译结束后的全部收尾 1.1 秒；跨卷 SampleApp 退出码 0、产物完整 | 克隆退化成全量复制，拖慢每次编译；跨卷时直接失败 | 实测 | 已实现 |
| A7.9 | 读 JSON：对象、数组、带转义和 Unicode 的字符串、数字、布尔、null；残缺或多余的输入返回空，不崩溃 | 结果包读坏了让 agb 崩溃或读错 | 单元 | 已实现 |
| A7.10 | 退出码 0、结果包里却记着错误（构建脚本打印 `error:` 后以 0 退出）→ 这些错误每条一行 `agb: error that did not fail the build: …`，没有编译器格式的 `error:` 行，汇总行 `errors=0`、警告数不变；退出码非 0 时错误照旧按编译器格式输出并计数 | 编译成功却报 `errors=2`，agent 或 CI 按错误数误判失败、去修不需要修的东西 | 单元；集成 | 已实现 |
| A8.1 | 配置解析：`键: 值` 和 `#` 注释；嵌套、未知键、非法大小 → 报错带行号；XDG 环境变量改变配置位置 | 语法边角（外层只抽一种错） | 单元 | 已实现 |
| A16.1 | 配置 `cache_limit: 1M`：编完这个项目的缓存超过 1M → 删掉它，汇总行前输出一行说明；没超过 → 不动；另一个项目的缓存不受影响；大小按二进制单位（1G = 1024³ 字节）、按占用的磁盘块计 | 超限不清理、清错项目、单位算错 | 单元；集成 | 已实现 |
| A11.1 | 项目文件解析：`ignored_inputs:` 下的 `- 路径`，引号和注释；绝对路径、`..`、仓库根目录、未知键、缩进不对 → 报错带行号 | 越出仓库的路径被同步，或写错被悄悄忽略 | 单元 | 已实现 |
| A11.2 | 从编译输出里找出槽内路径：引号包围、`文件:行:列`、括号里、含空格；前缀相似的其他目录不算；同一路径只算一次 | 漏掉缺失的文件，或把别的路径当成槽内路径 | 单元 | 已实现 |
| A25.1 | 解析 cargo 参数：跳过 `+工具链`、子命令前的选项和它们的值找到子命令，认得别名（`b`、`t`）；只放行编译类和 `clean`；`--target-dir`、`--message-format`（两种写法）连同值丢掉，`--` 之后不动；`--message-format=json` 插在子命令后；清空的 wrapper 变量恰好 4 个；`--manifest-path` 两种写法都能改指 | 丢参数时错位；把 `--` 后程序的参数当成 cargo 的；漏清某个 wrapper | 单元 | 已实现 |
| A21.1 | 用录制的真 cargo JSON：错误、警告取主 span，相对路径按工作区根补成绝对路径，依赖的绝对路径不动；note 和 "for more information" 不算诊断；没有 span 的链接错误照样算错误；非 JSON 行不是消息；测试框架输出里取出失败的测试和 panic 位置 | 位置差一行、指到次要 span、漏报链接错误 | 单元 | 已实现 |
| A21.2 | 按 Cargo 的 `fresh` 数复用和重编，本项目重编的包每个只记一次；包 id 省略包名（目录名即包名）时取目录名；汇总行字段 | 计数错、把依赖算成本项目；rust-sdk 上显示成 `0.0.0` | 单元 | 已实现 |
| A26.1 | 产物只取本项目包的 bin、staticlib、cdylib、dylib 的最终文件，不取 `.rlib`、`deps/`、构建脚本、测试框架、依赖 | 拷进中间产物或依赖 | 单元 | 已实现 |
| A9.1 | 依赖检查：Cargo.toml 里只有 workspace 内部 crate，core 没有任何依赖；加第三方依赖或让 core 依赖 sys 时失败 | 有人加了第三方依赖，或 core 开始做 IO | 集成 | 已实现 |
| A9.2 | 用例表核对：已实现的单元/集成用例必须有标了编号的测试，代码里的编号都在表里，tests/ 里的测试都有标注；任一不满足时失败 | 用例表和测试脱节 | 集成 | 已实现 |

## 价值验证

A10 的通过线。条件：同一台 Apple Silicon Mac，只装一个 Xcode，接电源；开跑前 load average 低于 CPU 核数，暂停其他重任务；Spotlight 排除槽和基准用的 DerivedData；基准和 agb 交替跑，各 3 次取中位数，同时记最大最小；每个 app 固定一个提交。主测 Swiftfin，NetNewsWire 复测 V1、V3、V4。通过线里标"假设"的数，第一次实测后按数据定。

| 编号 | 声明 | agb 的做法 | 基准 | 通过线 |
| --- | --- | --- | --- | --- |
| V1 | 新 worktree 首编（G2） | 槽已在提交 C 编过，新 worktree 也在 C | 裸 xcodebuild，新 DerivedData，不开编译缓存 | ≤ 基准的 50%（假设） |
| V2 | 差 1、10、50 个提交的新 worktree（G2） | 槽在 C，worktree 在 C~1、C~10、C~50 | 同 V1 | 每档都比基准快；记命中率 |
| V3 | A → B → A 切回（G5、S3） | 槽里依次编 A、B、A | 冷编 A | 切回 A 时可缓存任务命中率 ≥ 95%；产物可复现部分与冷编逐字节一致 |
| V4 | 同一 worktree 改一个文件（G1） | 改一个模块里的函数实现再编 | 裸 xcodebuild 用这个 worktree 自己热的 DerivedData | agb − 基准 ≤ 基准的 10%（假设） |
| V5a | 持久 runner 上的新分支（G4） | 槽已热；新的浅克隆，切到比主分支多 1 个提交的新分支 | 同 V1 | 同一个槽；耗时在 V2 的 C~1 档 ±20% 内；依赖 A18 |
| V5b | 临时 runner：槽每次都冷 | 新浅克隆加空槽 | 同一新克隆上裸 xcodebuild 冷编 | agb 多出的时间 ≤ 冷编的 5%（假设），否则 README 写明临时 runner 上不用 |
| V6 | 两个 worktree 轮流编的代价（N11） | 两个 worktree 交替 3 轮 | 各自热的 DerivedData | 只记录每次切换多花几秒，写进文档 |
| V7 | 被忽略的大目录每次同步的开销 | 声明一个 10 万文件、约 500 MB 的合成目录，不改代码连续同步 5 次 | 设计里 2 万文件首次 2.2 秒、之后 0.8 秒 | 记录耗时；超过 5 秒（假设）就提按 mtime 跳过的设计问题 |
