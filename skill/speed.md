# 给工程提速

文中的数字都来自同一个约 2400 个源文件的 iOS 工程（Xcode 27.2，Debug，模拟器），用来判断量级。换一个工程要重新测，不能直接照搬。

目录：先定位 · 测量 · 工程上值得改的 · 不值得做的

## 先定位

- 情形：要提速但还不知道慢在哪。效果：用户先看到一次空编译和一次改一个文件后的 `agb: phases` 与汇总行，再讨论改什么。空编译就是什么都不改，连跑两次，取第二次。
- `phases` 各段的意思：
  - `resolve`：xcodebuild 解析 SwiftPM 依赖、准备包，在闭源的 xcodebuild 里；
  - `graph`：计算 target 依赖图；
  - `plan`：生成或读取构建描述，读到上次结果时不到 1 秒；
  - `execute`：真正执行任务。
  - 前三段每次编译都要付，参考工程合计约 10 秒；空编译时 `execute` 里剩下的，就是每次都会重跑的任务。
- 情形：改一个文件后的编译和空编译耗时差不多。效果：用户知道瓶颈在每次都会付的固定开销，不在编译器。参考工程里空编译约 31 秒，改一个叶子模块文件约 33 秒，编译器真正处理这次改动只占几秒。
- 情形：`top misses` 在切回编过的提交后仍然很多。效果：查明是哪类输入不可复现（宏插件、生成文件），给出点名的 target。宏插件的自查方法见 agent-build README 的 P6 一节。

## 测量

- 情形：比较改动前后的耗时。效果：两边都用同一条命令、同一组环境变量，各跑至少两次，在机器负载接近时比较；负载看 `sysctl -n vm.loadavg`。原因：负载从 6 升到 20 时，同一次空编译从 22 秒变成 38 秒。
- 情形：两次编译之间环境变量变了，哪怕是和编译无关的变量。效果：把这一次算作"换环境后的首次编译"，不拿来和稳态比。原因：环境变量会进入构建描述的签名，也会进入部分任务的重跑判断。实测改一个无关变量后：
  - 构建描述重新生成，多 3～4 秒；
  - 依赖解析从约 4 秒变成约 9 秒；
  - 多跑 52 个 `ProcessInfoPlistFile` 和 1 次 `VerifyModule`。
- 情形：要临时加编译参数做诊断（`-stats-output-dir`、`-warn-long-function-bodies` 等）。效果：参数里的路径每次都相同，诊断完把参数撤掉，并预期加和撤各触发一次全量重编。原因：编译参数变了，所有 Swift 任务都会重编；路径每次不同，就每次都全量。
- 情形：想看每个阶段内部的耗时。效果：只看相对比例，绝对值以不加诊断参数的那次为准。原因：`-Xfrontend -fine-grained-timers` 会让内核态 CPU 涨十几倍。

## 工程上值得改的

每条都是改工程配置。动手前让用户看到代价，改完按「测量」对比前后。排序按用户最常遇到的那种编译：日常改几个文件就编，先看空编译的 `execute` 里每次都重跑的任务；换 worktree、切分支、冷编译慢，先看 `P4` 和 `top misses`。

| 情形 | 改法 | 收益与依据 | 代价 |
| --- | --- | --- | --- |
| 输出里有 `precondition P4 not met: <target>` | 这些 target 设 `SWIFT_ENABLE_EXPLICIT_MODULES=YES` | 这些 target 的 Swift 编译进入缓存，省在冷编译、换 worktree、切回编过的提交这类真要编译的场景；空编译本来就不编它们，省不到。`P4` 行消失、命中数上升即生效 | 少数旧工程打开后可能编不过，要逐个 target 修 |
| 输出里有 `precondition P5 not met: <文件>` | 生成器把产物写进构建目录；必须写在仓库里的，只在内容变化时才写 | 每次同步不再改回这些文件，依赖它们的 target 不再被连带重编；`P5` 行消失即生效 | 要改生成脚本 |
| 空编译的日志里每次都有 `PhaseScriptExecution` | 给脚本阶段声明输入和输出文件，让 Xcode 能判断跳过 | 参考工程 5 个脚本阶段每次约 5～8 秒 CPU | 输入输出声明不全时，该跑的会漏跑；要逐个核对 |
| 资源或代码生成器每次都改写生成文件，相应模块每次都重编 | 内容没变就不写文件 | 参考工程一个资源模块每次约 2 秒 | 小 |
| Debug 模拟器编译每次都 `GenerateDSYMFile` | Debug 配置设 `DEBUG_INFORMATION_FORMAT=dwarf`，Release 保持 `dwarf-with-dsym` | 每次约 4～5 秒；冷编译 CPU 基本不变（1251 秒对 1245 秒），模拟器上 LLDB 调试不受影响 | Debug 产物不再出 dSYM，需要用 dSYM 符号化 Debug 崩溃的流程会受影响 |
| 仓库内源码编译的 framework 开着 `BUILD_LIBRARY_FOR_DISTRIBUTION=YES` | 只给对外发布二进制的 target 打开 | 参考工程里开着它的 framework 每次空编译都重新生成模块接口，约 2.6 秒；完整原因没查完，关掉后要实测确认 | 对外发二进制的 target 必须保留 |
| 开着 `ENABLE_MODULE_VERIFIER=YES` | 只在发布前的检查里打开 | `VerifyModule` 重跑一次约 5 秒 CPU | 模块头文件的问题晚一些才发现 |
| scheme 的 pre-action 每次都跑 | 脚本先判断"已是最新"再提前退出 | pre-action 串行地跑在规划之前；参考工程做到了约 1.4 秒 | 小 |

## 不值得做的

- 情形：想从编译器内部提速（类型检查、生成代码、调试信息）。效果：先用 `-Xfrontend -warn-long-function-bodies=100` 和 `-warn-long-expression-type-checking=100` 查一次热点。没有热点就不往这个方向投入。依据：参考工程冷编译里超过 100 毫秒的类型检查只有 8 处，合计 1.4 秒；生成模块接口时默认已经跳过函数体。
- 情形：想加 `-disableAutomaticPackageResolution`、`-skipPackageUpdates`、`-skipPackagePluginValidation`、`-skipMacroValidation` 来缩短 `resolve`。效果：命令里已经有 `-onlyUsePackageVersionsFromResolvedFile` 时不加。依据：参考工程实测，这几个参数的变化都在噪声范围内。
- 情形：想自己管 DerivedData（换路径、复制、搬家）。效果：交给 agb 的槽。依据：预编译模块（PCM）里记着模块缓存的绝对路径，复制到别处的 DerivedData 会编译失败。
