# 本地 Codex 动态模型路由设计

**状态：** 待评审设计稿
**日期：** 2026-09-06
**适用范围：** 本地 Codex Desktop 通过 `example-managed` 类 SSH 通道连接远端 macOS Codex App Server 的场景
**不适用范围：** Across Agents Assistant（AAA）产品路由、通用 OpenAI API 网关

## 1. 决策摘要

现有 `codex-managed-channel` 可以作为动态模型路由的稳定承载层。它已经把 Codex Desktop 的 App Server 协议通过受限 SSH 通道传递给远端官方 Codex App Server，并保持 Thread 与 Turn 语义不变。

动态模型路由不应直接写进公开通道的生命周期管理核心，也不应改造 Codex Desktop。推荐新增一个私有、可选的 `codex-model-router` 协议侧车，通过独立 SSH 别名 `example-auto` 启用：

```text
Codex Desktop
    │ App Server protocol
    ▼
OpenSSH alias: example-auto
    │ restrictive forced-command
    ▼
codex-managed-entry
    │ starts a configured Codex-binary shim
    ▼
codex-model-router
    ├── delegates official app-server startup unchanged
    ├── intercepts only complete turn/start messages
    ├── asks Luna to classify the first substantive turn
    ├── validates the selected tier against model/list
    ├── rewrites only model and effort
    └── delegates to official codex app-server proxy
    ▼
official Codex app-server
    ▼
same Thread history, remote tools, skills, plugins and workspace
```

第一版采用“Thread 级一次定级并固定模型”，而不是每轮自动升降级。后续可以在 Turn 边界增加受控升降级，但不允许在正在执行的 Turn 中热切换，也不允许在工具已经产生副作用后透明重放整个 Turn。

## 2. 背景与目标

目标是在用户通过本地 Codex Desktop 发起新任务时，先用 Luna 对任务复杂度进行一次低成本定级，再把真实任务交给合适的模型执行，同时保留官方 Codex 的 Thread、工具调用、技能、插件、审批和远端工作区能力。

需要达成：

- 用户继续使用 Codex Desktop，不需要复制 Prompt 到另一个应用。
- 正式任务的所有 Turn 继续使用同一个真实 Codex Thread；分类操作使用与正式 Thread 隔离的临时连接和临时 Thread。
- 正式任务只改变 `turn/start` 中的模型与推理强度，不复制或重建历史消息。
- 现有 `example-managed` 保持完全透传，随时可作为人工选模和回滚通道。
- 分类失败不得阻塞正常工作。
- 不记录原始 Prompt、完整会话、凭证或工具输出。
- 模型名单与推理强度从当前 App Server 能力获取，不把当下的模型 ID 当作永久事实。

## 3. 现有通道提供的能力

`codex-managed-channel` 的运行链路是：

```text
Codex Desktop
  -> OpenSSH managed alias
  -> remote sshd forced-command
  -> codex-managed-entry
  -> official codex app-server proxy
  -> per-connection Unix socket
  -> official codex app-server
```

现有实现具备三个关键条件：

1. `codex-managed-entry` 负责连接级隔离、进程组回收、空闲策略和 App Server 生命周期，不替代官方 Agent runtime。
2. `ProtocolObserver::observe` 只解析生命周期信息，最终返回收到的原始字节；`forward_client_chunk` 再把这些字节写给官方代理。
3. 官方 App Server 支持在 `thread/start` 和 `turn/start` 指定模型；`turn/start` 还可以逐 Turn 覆盖模型和推理强度。

因此，同一 `threadId` 下可以在下一个 Turn 使用不同模型，而不需要把历史消息重新拼装成 API `messages` 数组。

## 4. 与原始 API 网关方案的区别

原始方案中的 Python `SmartSessionRouter` 适用于自建 Chat API，但不能直接等同于 Codex Desktop 的本地任务路由。落地到 Codex 时需要修正以下假设：

| 原始假设 | 本地 Codex 的处理方式 |
|---|---|
| 网关自己维护 `history_messages` | Thread 历史由官方 App Server 维护，路由器不得复制会话状态 |
| 直接调用 Chat Completions API | 通过官方 Codex App Server/代理复用现有认证和运行环境 |
| 每轮都让 Luna 分类 | 第一版只在新 Thread 的第一条真实 Turn 分类一次 |
| 关键词命中就直接上 Sol | 关键词只可作为风险下限，不能代替结构化分类和实际评估 |
| Terra 报错后自动用 Sol 重跑 | 仅在确认没有工具副作用时允许；默认不透明重放 |
| 固定模型字符串 | 启动时通过 `model/list` 校验并按能力映射 |
| 固定 100–200 ms 延迟、固定逃逸率目标 | 这些数值必须通过本机真实测量建立，设计阶段不作保证 |

## 5. 方案选择

### 5.1 推荐：独立自动路由别名与协议侧车

新增 `example-auto`，使用独立 SSH key 和 restrictive forced-command。该路径启用 `codex-model-router`；原 `example-managed` 不变。

优点：

- 不需要修改 Codex Desktop。
- 手动通道和自动通道物理隔离，回滚直接切换别名。
- 路由器可以看到 SSH 解密后的 App Server 协议消息。
- 分类进程可以纳入同一连接的进程组清理范围。
- 公开通道仍保持默认零改写语义。

代价：

- 路由器必须正确处理 App Server 协议分帧、请求 ID 和版本变化。
- 自动别名下由路由策略拥有模型选择权；人工选模使用原别名。

### 5.2 备选：修改 Codex Desktop 或使用 Hook

若未来 Codex Desktop 原生支持 `model: auto` 或可插拔路由器，这是长期最干净的方案。当前 `UserPromptSubmit` Hook 可以观察 Prompt 和追加上下文，但没有覆盖本轮模型的正式输出字段，因此不能单独完成无感路由。

### 5.3 不推荐：把分类逻辑写入通道核心

直接修改 `ProtocolObserver` 进行模型调用和消息改写，会把传输生命周期组件变成 Agent 网关，破坏当前“协议字节不变”的安全边界，也会让公开通道承担模型策略、凭证、超时和计费责任。

### 5.4 不推荐：完全绕过 Codex 的通用 API 网关

直接使用 Python/Node OpenAI API 网关需要自行维护会话历史、工具、审批、插件和工作区状态，无法自然复用本机 Codex 的完整运行环境。

## 6. 推荐实现结构

### 6.1 `codex-model-router` 进程

建议使用 Rust，与现有通道保持相同的进程和协议处理方式。它作为固定路径的 Codex binary shim：

- 收到 `app-server --listen ...`、`--version` 等命令时，使用固定绝对路径直接执行官方 Codex。
- 收到 `app-server proxy --sock ...` 时，启动协议路由模式，并把非目标消息原样转发给官方代理。
- 启动 Luna 分类调用时，使用同一隔离 App Server 的独立代理连接，避免把分类请求和响应注入 Desktop 的协议流。
- 所有子进程继承当前连接的进程组，确保原通道的 EOF、空闲和归档回收仍然有效。
- 不接受任意命令、任意可执行路径或来自 `SSH_ORIGINAL_COMMAND` 的自由参数。

现有 `CODEX_MANAGED_CODEX_BIN` 配置点可用于验证这一组合方式，但正式实现必须固定并验证真实 Codex 路径，防止 shim 递归调用自身或扩展为任意命令执行入口。

### 6.2 路由策略

分类器只返回抽象等级，不直接返回任意模型 ID：

```json
{
  "tier": "balanced",
  "confidence": 0.87,
  "reason_codes": ["multi_file_implementation"],
  "risk_flags": []
}
```

允许的等级：

- `economy`：目标清晰、范围小、低风险、步骤确定。
- `balanced`：常规开发、分析、文档和多文件实现；默认等级。
- `deep`：架构设计、跨仓库工作、复杂调试、高风险变更或长链推理。

策略层再把等级映射到 `model/list` 返回的当前可用模型及其受支持推理强度。初始意图可以是 Luna、Terra、Sol 三档，但模型 ID 不能成为持久协议。

### 6.3 分类输入

第一版分类器只接收：

- 当前用户 Prompt。
- 附件的类型和数量，不包含不必要的文件正文。
- 当前工作目录的非敏感类别信息。
- 是否为新 Thread、是否为恢复的 Thread。
- 路由策略版本。

默认不向分类器发送：

- 完整 Thread 历史。
- 工具输出、终端日志或文件内容。
- 凭证、环境变量、Cookie 或审批内容。

分类连接必须是临时或可删除的独立 Thread，并禁止工具执行。若当前 App Server 版本无法可靠禁止分类 Thread 的工具调用，第一版不得启用该分类路径。

### 6.4 Thread 状态

路由器维护：

```text
threadId -> tier, resolved_model, effort, policy_version, decided_at
```

不保存原始 Prompt。新 Thread 在第一条实质性 `turn/start` 到达时分类；恢复已有 Thread 时优先复用该 Thread 最近一次有效决策。若状态不存在，则在恢复后的第一条新 Turn 重新分类。

路由元数据存储在受限的本地运行目录，采用容量和保留期上限。删除或归档 Thread 时同步移除或过期对应路由记录。

### 6.5 协议改写边界

路由器只能改写目标 `turn/start` 的：

- `params.model`
- `params.effort`

除目标完整消息外，其他流量必须逐字节透传。路由器必须正确处理：

- 一条消息被拆成多个读取块。
- 多条消息合并在一个读取块。
- JSONL 和当前代理使用的帧格式。
- 未知字段和新协议字段。
- Desktop 后台请求与真实用户 Turn 的区别。

解析失败时不得猜测或部分改写，应记录不含 Prompt 的 `routing_bypassed` 事件并把原消息完整透传。

## 7. 请求流程

### 7.1 新 Thread

1. Desktop 通过 `example-auto` 建立 SSH 连接。
2. forced-command 启动原 `codex-managed-entry`，并通过固定 shim 启动官方 App Server 和代理。
3. 初始化、`model/list`、`thread/start` 等消息正常转发；路由器缓存能力信息和新 `threadId`。
4. 第一条真实 `turn/start` 到达后，路由器暂停这一条完整消息，但继续遵守总超时和断开处理。
5. 路由器通过独立 App Server 连接让 Luna 返回严格结构化等级。
6. 本地策略应用风险下限、置信度和模型可用性规则。
7. 路由器只改写 `model` 与 `effort`，随后发送原始 Turn。
8. 决策绑定到 `threadId`；后续 Turn 使用该绑定模型。

### 7.2 分类失败

1. Luna 超时、输出无效或连接失败。
2. 路由器使用当前 `model/list` 中配置的平衡型默认模型；若能力列表不可用，则保留 Desktop 原请求的模型。
3. 正式 Turn 继续执行。
4. 日志只记录失败类别、耗时、策略版本和最终模型。

### 7.3 目标模型不可用

- `economy` 或 `balanced` 目标不可用：选择可用的平衡型默认模型。
- `deep` 目标不可用：选择当前可用的最高能力模型及其受支持 effort；不得静默降到 economy。
- 没有安全映射：保留 Desktop 原模型，并向本地诊断日志写入可操作原因。

### 7.4 Turn 间升降级

App Server 允许下一次 `turn/start` 使用不同模型，因此技术上可以在同一 Thread 内升降级。该能力放到第二阶段：

- 只在 Turn 边界重新判断。
- 高风险信号可以立即升级。
- 降级需要连续低复杂度信号和滞回规则，避免频繁抖动。
- 正在运行的 Turn 不热切换。
- 发生工具调用、文件写入或外部操作后，不自动重放失败 Turn。
- 需要重试时，必须确认失败发生在任何副作用之前，或者显式请求用户确认。

## 8. 自动与手动模式

采用两个 SSH 别名明确语义：

- `example-managed`：现有完全透传通道，Desktop 模型选择原样生效。
- `example-auto`：自动路由通道，路由策略拥有模型和 effort 的最终选择权。

这样无需猜测 Desktop 发送的模型字段是用户主动选择还是客户端默认值，也不会在用户明确手动选模时暗中覆盖。

## 9. 安全与隐私要求

- `example-auto` 使用独立密钥和独立 restrictive forced-command。
- forced-command 只启动固定绝对路径的路由入口，不执行任意原始 SSH 命令。
- 路由器和分类器不得读取或记录 API key、Codex 登录令牌、Cookie、环境变量或完整 Prompt。
- 不引入公共网络监听；继续使用 Unix socket 和 SSH。
- 分类调用复用官方 Codex 认证边界，不额外复制凭证到配置文件。
- 分类线程禁止工具、MCP、浏览器、Computer Use 和写文件能力。
- 路由状态目录权限必须在创建时即为用户私有。
- 日志采用容量限制和轮转策略。
- 原 `example-managed` 是永久保留的安全回滚路径。

## 10. 可观测性

允许记录：

- 匿名或本机范围的决策 ID。
- Thread ID 的本地哈希或受限标识。
- 策略版本、分类器版本。
- tier、最终模型、effort、confidence。
- 分类耗时、总路由耗时。
- fallback/bypass 原因码。
- 当前 App Server/Codex 版本。

禁止记录：

- 原始 Prompt 或摘要。
- 完整 Thread 历史。
- 工具参数和输出。
- 文件内容和绝对用户文档路径。
- 凭证或身份信息。

第一阶段不设定未经验证的“Sol 逃逸率”“固定 TTFT”目标。先通过 shadow mode 建立本机基线，再决定质量、延迟和成本阈值。

## 11. 失败策略

| 失败 | 默认行为 |
|---|---|
| 分类超时 | 使用平衡型默认模型，继续正式 Turn |
| Luna 不可用 | 使用本地风险下限与平衡型默认模型 |
| 分类 JSON 无效 | 丢弃分类结果，不从自由文本猜测 |
| `model/list` 不可用 | 保留 Desktop 原模型 |
| 协议消息无法完整解析 | 原字节透传 |
| 路由器自身崩溃 | 连接失败并提示切换 `example-managed`；不得留下孤儿进程 |
| 目标模型在 Turn 启动前拒绝 | 仅在无副作用时选择允许的替代模型 |
| Turn 执行中失败 | 不透明重放；由用户或后续 Turn 决定升级 |

## 12. 分阶段落地

### 阶段 0：协议与能力验证

- 在当前 Codex 版本上确认 `model/list`、`thread/start`、`turn/start.model` 和 `turn/start.effort` 的实际协议形态。
- 验证同一 `threadId` 在两个连续 Turn 使用不同模型仍保持上下文。
- 验证独立代理连接可以在同一隔离 App Server 上运行临时分类 Thread。
- 验证分类 Thread 可以可靠禁止工具调用。

### 阶段 1：Shadow mode

- 新建 `example-auto` 和路由器，但不改写模型。
- Luna 对第一 Turn 分类；仅记录结构化结果和延迟。
- 用一组本地代表性任务人工标注正确 tier，评估误判。
- 确认路由器退出后没有残留进程、socket、临时 Thread 或无限增长日志。

### 阶段 2：Thread 级自动路由

- 启用第一 Turn 改写。
- 决策绑定 Thread，后续 Turn 保持稳定。
- 加入超时、无效输出、模型不可用和协议升级回退。
- 保留 `example-managed` 一键回滚。

### 阶段 3：Turn 边界升降级

- 只对新 Turn 重新评估当前输入。
- 加入升级立即、降级滞后的防抖策略。
- 将副作用检测和不可自动重放规则作为发布阻断条件。

## 13. 测试与验收

### 单元测试

- JSONL/帧解析、拆包、粘包和超大消息。
- 未知字段、协议扩展和无效 JSON。
- 只改写允许的两个字段。
- tier 映射、置信度、风险下限和 fallback。
- Thread 状态新增、恢复、归档、删除和过期。

### 集成测试

- 使用假 App Server 验证请求 ID 隔离和分类连接不泄漏到 Desktop。
- 使用假 Luna 验证超时、错误 JSON、不可用模型。
- 验证 shim 对非 proxy 命令只调用固定官方 Codex。
- 验证所有子进程位于可回收的连接进程组。
- 验证 `example-managed` 的字节级行为完全不变。

### 本机验收

- 在临时工作区创建新 Thread，并证明第一 Turn 使用路由模型。
- 在同一 Thread 发起第二 Turn，证明上下文连续且模型保持绑定。
- 使用受控无副作用任务验证 Turn 间模型切换。
- 断开、归档和空闲回收后检查没有任务归属的进程与 socket。
- 检查日志中不存在原始 Prompt、凭证或工具输出。
- 升级 Codex 后重新运行协议兼容性套件。

### 发布准入条件

- 分类失败不会阻止正式任务。
- 不会在已有工具副作用后自动重放 Turn。
- 不会改变 `example-managed`。
- 路由关闭时行为等同官方透传。
- 路由启用时仍使用官方 App Server、Thread 和工具环境。
- 原别名可立即完成回滚。

## 14. 明确不做

- 不修改或注入 Codex Desktop 二进制。
- 不使用退役的 Codex memory 文件保存路由结论。
- 不把完整历史重新发送给 Luna。
- 不让分类模型执行工具或访问仓库。
- 不用关键词列表作为唯一复杂度判断。
- 不在活跃 Turn 中切换模型。
- 不在不确定是否产生副作用时自动重跑。
- 不将 Luna、Terra、Sol 的当前名称固化为长期公共协议。
- 不把该能力加入 AAA 的任务模型或界面。

## 15. 风险与缓解

| 风险 | 缓解 |
|---|---|
| App Server 协议仍在演进 | 版本探测、未知字段透传、协议回归测试、解析失败不改写 |
| 分类增加首轮延迟和额外用量 | 只分类第一 Turn、严格短输出、超时回退、先做 shadow 测量 |
| Luna 低估复杂度 | 风险下限、低置信度回到 balanced、评测集持续校准 |
| 频繁升降级造成行为漂移 | 第一版 Thread 固定；第二阶段加入滞回 |
| 自动重试产生重复副作用 | 禁止默认重放执行中 Turn |
| 路由器扩大 SSH 攻击面 | 独立 key、固定 forced-command、固定可执行路径、无任意参数 |
| 分类数据泄漏到日志 | 只记结构化元数据，不记 Prompt 和内容摘要 |
| Codex 升级导致字段变化 | 升级后先跑兼容测试；失败时原样透传或切回手动别名 |

## 16. 后续实施入口

设计评审通过后，下一步应编写独立实施计划，优先完成阶段 0 和 shadow mode。不要直接从完整动态升降级开始。

建议未来实现归属：

- 公开 `codex-managed-channel`：继续负责受限 SSH、隔离 App Server、协议透传和生命周期回收。
- 私有 `codex-model-router`：负责分类、策略、协议目标字段改写和路由状态。
- 本地配置：负责 tier 到当前可用模型的映射、超时和默认模型。

## 17. 来源与证据

本地源码与设计：

- `docs/architecture.md`：通道是 transport boundary，并保持官方 App Server 不变。
- `docs/superpowers/specs/2026-09-05-public-release-design.md`：SSH、forced-command、官方代理、每连接 Unix socket 与生命周期设计。
- `src/protocol.rs` 中 `ProtocolObserver::observe`：观察协议后返回原始字节。
- `src/supervisor.rs` 中 `forward_client_chunk`：观察完成后把相同字节写给代理。
- `src/supervisor.rs` 中 `spawn_proxy`：启动官方 `codex app-server proxy`。
- 2026-09-06 本机运行检查：已安装的 `codex-managed-entry` 正在通过受限运行目录中的 Unix socket 连接官方代理。

官方能力文档：

- Codex App Server：https://learn.chatgpt.com/docs/app-server
- Codex Hooks：https://learn.chatgpt.com/zh-Hans/docs/hooks
- Codex Models：https://learn.chatgpt.com/docs/models

这些外部能力可能随 Codex 版本变化。实现与验收时以目标机器当前版本的 `model/list` 和实际协议探测为准。
