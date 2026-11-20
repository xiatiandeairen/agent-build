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
| A4 | 编译前后调用方 worktree 的文件、index、HEAD、git 配置不变；origin 指向不可达地址时照常完成；调用方没初始化的子模块在槽里也没有 | 改到用户仓库、偷偷联网、替工程初始化：这些错不会让编译失败 | 集成 | 已实现 |
| A5 | 同项目两个调用同时开始 → 后者打印"在等 <worktree>"，前者结束后才开始，两个都成功；不同项目不互相等；持有者收到 SIGTERM → xcodebuild 先停、再释放锁；持有者被 kill -9 → 后来者等残留的 xcodebuild 结束后再接管（锁是 flock，文件描述符交给 xcodebuild 继承，内核在最后一个持有进程退出时释放，不需要判断失效锁） | 两个编译同时写一个槽，结果被破坏且难以复现 | 集成 | 已实现 |
| A6 | 假 xcodebuild 打印报错并以 65 退出 → 报错原样输出（路径已改写），退出码 65，只调用一次；槽目录不可写 → 退出码 125，不调用 xcodebuild；完整日志在 logs/ | 掩盖失败、偷偷重试、退出码让 CI 误判 | 集成 | 已实现 |
| A7 | 假 xcodebuild 配合用真 Xcode 录制的结果包 JSON（`tests/fixtures/xcresult`，由假 xcresulttool 提供）→ 编译期间终端只有一行起始提示，原始输出只进日志；编完依次输出错误（`路径:行:列: error: …`，路径已改写）、失败的测试、阶段耗时、P4、未命中最多的 target、P5、结果包和产物路径、汇总行（含 errors、warnings）；失败但读不到结果包时，输出原始输出的最后 30 行；下次同步把被改写的文件改回 | G6 的输出整体对不上：缺行、顺序乱、刷屏；失败时看不到原因 | 集成 | 已实现 |
| A8 | 没有配置文件 → 每个项目的缓存上限 20G，缓存目录在这个项目的槽里；写 `cache_limit: 10G` → 传给 xcodebuild 的是 10G；配置写错 → 退出码 64，指出行号 | 配置没生效或写错时被悄悄忽略 | 集成 | 已实现 |
| A9 | `just check` 一条命令跑完格式、clippy（禁止 unwrap/expect/panic，函数 ≤ 60 行）、全部单元和集成、单文件 ≤ 400 行、无第三方依赖、用例表核对 | 代码质量约束悄悄退化 | 实测 | 已实现 |
| A11 | 工程依赖被 git 忽略的文件（P8）：没声明时编译失败 → 输出点名这个被忽略的文件和声明方法，被跟踪的文件报错不点名；在 `.agent-build.yaml` 声明后 → 文件、目录、可执行位、符号链接原样进槽，只重写变化的文件，worktree 里删掉的槽里也删；从声明里去掉 → 槽里删掉；声明写错 → 退出码 64 带行号，不编译 | 编出来的东西用了过时的被忽略文件；失败时报错指向 worktree 里明明存在的文件，让人找错方向 | 集成 | 已实现 |
| A12 | 样例 SampleWorkspace 放在仓库的 `ios/` 子目录（`-workspace`，App 依赖一个本地 SwiftPM 包，包里有 Swift 和 ObjC 两个 target，ObjC 公开头被 Swift 引用），整个仓库放在路径含空格和中文的目录下；在 `ios/` 里 `agb xcodebuild build -workspace SampleWorkspace.xcworkspace -scheme App -destination 'generic/platform=iOS Simulator'` → 退出码 0，产物目录里有 App.app；不改再编 → `miss=0`；ObjC 公开头加一个方法声明再编 → 退出码 0；造一个编译错误 → 错误行指向 worktree 里含空格和中文的路径；`agb xcodebuild -resolvePackageDependencies -workspace SampleWorkspace.xcworkspace -scheme App` → 退出码 0，没有 `products=` 行 | 真 xcodebuild 只跑过 `-project` 纯 Swift；公开样本里 workspace 50%、子目录 34%、ObjC 混编 39%、本地包 28% | 冒烟 | 已实现 |
| A13 | 在 SampleWorkspace 的包目录里 `agb xcodebuild test -scheme <包>-Package -destination 'id=<iOS 模拟器>' -only-testing:<一个改成失败的测试>` → 退出码 65，输出 `agb: test failed: <该测试>`；改回后退出码 0；回 `ios/` 不改代码再编 App → `miss=0`（切换容器会让部分任务重做，但全部命中编译缓存，不重新编译） | 在包目录里只给 `-scheme` 的 CI 占 18%；workspace 和包两种容器轮流用同一个 DerivedData，互相作废时每次切换都重编，G1 悄悄失效 | 冒烟 | 已实现 |
| A14 | SampleApp：`agb xcodebuild build-for-testing -project SampleApp.xcodeproj -scheme CoreTests -destination platform=macOS` → 产物目录里有 `.xctestrun`；`agb xcodebuild test-without-building -xctestrun <它> -destination platform=macOS` → 退出码 0；一个测试失败时输出 `agb: test failed: …` | 14% / 11% 的 CI 用两段式；agb 给第二段注入的 `-derivedDataPath` 和结果包，真 xcodebuild 是否接受没验证过 | 冒烟 | 已实现 |
| A17 | SampleApp：`agb xcodebuild archive -project SampleApp.xcodeproj -scheme App -destination 'generic/platform=iOS' CODE_SIGNING_ALLOWED=NO` → 退出码 0，输出的 `archive=` 路径存在，里面有 `Products/Applications/App.app` | 21% 的 CI 直接 archive；注入的 `-archivePath` 和 archive 路径只在单元层测过 | 冒烟 | 已实现 |
| A18 | 调用方是 `--depth 1` 浅克隆 → 建槽成功，同步后逐字节相同；调用方再 `git fetch --depth 1` 到更新的提交 → 再次同步成功；删掉调用方，同一 origin 的新浅克隆（更新的提交）→ 用同一个槽同步成功，退出码 0 | 67% 的 CI 检出是浅克隆；槽从调用方本地 fetch，浅仓库之间互相 fetch 可能缺对象或被拒；出错时持久 runner 上每次 CI 都以 125 失败，G4 落空 | 集成 | 已实现 |
| A10 | my-app 上：新 worktree 首编、A → B → A 切回时的缓存命中、改 ObjC 公开头后增量不报错、产物与从零编译的可复现部分逐字节一致、CI 新分支增量；记录耗时和命中数 | 验证 Xcode 在真实工程上的表现，样例工程证明不了 | 实测 | 待实现 |

## 内层细节

| 编号 | 情形 → 预期 | 防什么错 | 验证 | 状态 |
| --- | --- | --- | --- | --- |
| A1.1 | 第一个词不是 `xcodebuild`（`xcrun xcodebuild`、绝对路径、`cargo`、`swift`）、带 `-exportArchive`、没有参数 → 退出码 64 并提示正确写法或用法 | 把别的命令当编译执行 | 单元 | 已实现 |
| A1.2 | 带 `-derivedDataPath`、`-archivePath`、`-resultBundlePath` → 每个丢掉并各提示一行，其余参数顺序不变 | 丢参数时多吃或少吃一个值，参数错位 | 单元 | 已实现 |
| A1.3 | 注入的参数恰好是缓存设置 + 槽的 `-derivedDataPath`，archive 时加槽里的 `-archivePath`；不含其他编译设置 | 越界改设置；漏注入 `-derivedDataPath` 而写进 Xcode 默认目录 | 单元 | 已实现 |
| A1.4 | 在不是 git 仓库的目录运行 → 退出码 64，说明需要 git 仓库 | 在任意目录乱建槽 | 集成 | 已实现 |
| A1.5 | 在 worktree 子目录里运行 → xcodebuild 的工作目录是槽里同一子目录；输出路径改写成 worktree 子目录下的路径 | 34% 的工程在子目录，这条路径零测试；算错时 xcodebuild 在槽根目录运行，找不到工程 | 集成 | 已实现 |
| A1.6 | 带 `-resolvePackageDependencies` → 按信息命令处理：注入槽的 `-derivedDataPath`（包解析进槽），不注入结果包，原样输出，不给产物路径 | 11% 的 CI 用它；落进编译分支会打印不存在的产物目录，并去读不存在的结果包 | 单元 | 已实现 |
| A1.7 | `-workspace`、`-project`、`-xctestrun` 的值是 worktree 里的绝对路径（含经符号链接的写法）→ 改写成槽里的同一路径，各提示一行；worktree 之外的路径不动 | 7% 的 CI 用 `$GITHUB_WORKSPACE/…` 指工程；原样传过去编的是 worktree 里的工程，同步白做，G1/G2 悄悄失效 | 单元；集成 | 已实现 |
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
| A7.7 | 按动作给出路径：每次都给槽里的结果包；build、test 成功 → 产物目录并说明有效期；archive 成功 → 槽里的 .xcarchive；编译失败只给结果包（产物可能是旧的或不存在）；信息命令不给 | 给出不存在或不对的路径 | 单元 | 已实现 |
| A7.8 | 日志只保留最近若干份，超出的按时间删旧的 | 日志无限增长 | 单元 | 已实现 |
| A7.9 | 读 JSON：对象、数组、带转义和 Unicode 的字符串、数字、布尔、null；残缺或多余的输入返回空，不崩溃 | 结果包读坏了让 agb 崩溃或读错 | 单元 | 已实现 |
| A8.1 | 配置解析：`键: 值` 和 `#` 注释；嵌套、未知键、非法大小 → 报错带行号；XDG 环境变量改变配置位置 | 语法边角（外层只抽一种错） | 单元 | 已实现 |
| A16.1 | 配置 `cache_limit: 1M`：编完这个项目的缓存超过 1M → 删掉它，汇总行前输出一行说明；没超过 → 不动；另一个项目的缓存不受影响；大小按二进制单位（1G = 1024³ 字节）、按占用的磁盘块计 | 超限不清理、清错项目、单位算错 | 单元；集成 | 已实现 |
| A11.1 | 项目文件解析：`ignored_inputs:` 下的 `- 路径`，引号和注释；绝对路径、`..`、仓库根目录、未知键、缩进不对 → 报错带行号 | 越出仓库的路径被同步，或写错被悄悄忽略 | 单元 | 已实现 |
| A11.2 | 从编译输出里找出槽内路径：引号包围、`文件:行:列`、括号里、含空格；前缀相似的其他目录不算；同一路径只算一次 | 漏掉缺失的文件，或把别的路径当成槽内路径 | 单元 | 已实现 |
| A9.1 | 依赖检查：Cargo.toml 里只有 workspace 内部 crate，core 没有任何依赖；加第三方依赖或让 core 依赖 sys 时失败 | 有人加了第三方依赖，或 core 开始做 IO | 集成 | 已实现 |
| A9.2 | 用例表核对：已实现的单元/集成用例必须有标了编号的测试，代码里的编号都在表里，tests/ 里的测试都有标注；任一不满足时失败 | 用例表和测试脱节 | 集成 | 已实现 |

