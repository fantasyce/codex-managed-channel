# Managed SSH 有界恢复：独立验收报告

日期：2026-09-15。验收执行者独立于产品实现；只编写探针、执行测试和提出缺陷，没有修改产品源码。

## 2026-09-17 增量验收：原位 unsubscribe 回收

在未修改已安装 managed 入口的临时环境中，使用同一官方 Codex 0.154.0
新增并执行了三项场景：

| 场景 | 验证内容 | 结果 |
|---|---|---|
| idle-empty | 空连接跨过 idle/drain 后 worker 和协议连接保持可用 | 通过 |
| idle-thread | 注入 `thread/unsubscribe` 后同 worker 继续服务，MCP、FD 和 PIPE 收敛 | 通过 |
| idle-thread-stubborn | MCP 忽略 SIGTERM 时，官方超时强杀路径仍完成关闭 | 通过 |

两个 thread 场景均收到 `idle_unsubscribe_completed`，随后同一连接上的
`thread/loaded/list` 返回空列表；没有出现 worker 替换、SSH/协议断开、测试强杀
介入或残留自有进程。另有一轮故意错误的 JSONL 注入复现了 unsubscribe 超时；
修正为 masked WebSocket text frame 后重新验收转绿。补充多线程协议验收进一步
确认：普通 idle unsubscribe 超时不能升级为断联，只有 FD 阈值保护路径保留
断联兜底。

增量版本重新执行 `cargo fmt --check`、严格 Clippy、67 项 Rust 测试和 23 项
Python 测试，均为零失败。

## 此前隔离验收结论

此前隔离阶段的冻结二进制通过 **17 项真实官方运行时入口场景、59 项 Rust 测试、23 项 Python 测试、格式检查和严格 Clippy 检查**。全部最终场景自然完成资源回收，没有使用测试强杀兜底。部署版本增加 unsubscribe 响应诊断及 idle 超时保持连接策略后，回归数量更新为上文记录的 67 项 Rust 测试和 23 项 Python 测试。

本节以下内容保留隔离工程验收的边界，不包含任何真实机器、账号、连接别名、进程号或运行路径。

独立审查结论：在本报告明确约定的“隔离环境开发验收”范围内，**未发现尚未关闭的阻断项**。已经复现的失败均有最终版本的通过证据。真实 SSH、Desktop 及真实工具运行仍是单独的上线门槛，不应因此标记为完成。

运行位置：临时开发目录中的 debug 入口，配合机器已有的官方 Codex 0.154.0。模型请求全部到临时 localhost mock 服务，认证值是假值。HOME、CODEX_HOME、配置、插件桩、会话和日志全部隔离。没有修改已安装入口、SSH 配置、真实凭据、真实任务或官方二进制。

## 最终场景结果

下列场景均使用合成测试数据。每项进程退出码均为 0。

| 场景 | 验证内容 | 结果 |
|---|---|---|
| contention | 相同身份第二连接被拒绝，第一 worker 与协议仍正常 | 通过 |
| reconnect | 断线后同 worker、同 turn 完成，模型请求只有一次 | 通过 |
| isolation | 两个合成身份使用不同 worker，关闭一个不影响另一个 | 通过 |
| expiry | 单次断连期限耗尽回收旧 worker，然后才允许新 worker | 通过 |
| lifetime | 反复重新附着不能延长绝对寿命，不允许新旧 worker 共存 | 通过 |
| crash | owner 被 SIGKILL 后 guardian 回收 worker，允许安全替换 | 通过 |
| budget | 反复重新附着不能重置累计断连预算 | 通过 |
| offline | 断连期间完成，重连查询到原 turn 完成，没有重复模型请求 | 通过 |
| startup-crash | socket 就绪前 owner 被 SIGKILL，正在启动的 worker 仍被回收 | 通过 |
| idle-active | 活跃模型 turn 跨过空闲阈值及 drain 后仍运行 | 通过 |
| idle-empty | 官方运行时真正空闲时出现 idle_reclaim 并完成回收 | 通过 |
| hard-fd | FD 数超过故意降低的硬阈值时强制终止 | 通过 |
| server-crash | server 先被 SIGKILL，已登记的 setsid 脱组后代仍被回收 | 通过 |
| stop-isolation | `--stop-client` 只停止指定合成身份，另一个身份继续响应 | 通过 |
| approval | 待审批跨空闲阈值、断连；同请求重新投递，明确取消后原 turn interrupted | 通过 |
| user-input | Plan 模式待用户输入跨空闲阈值、断连；同请求重新投递，明确回答后原 turn completed | 通过 |
| guardian-crash | guardian 单独被 SIGKILL，仍活着的 owner 兜底回收 worker | 通过 |

其中审批测试始终没有发送接受审批，只发送 `cancel`，没有执行被审批命令。用户输入测试回答的是合成选项 `Skip`；两次 mock 模型请求分别为工具调用前和回答后的正常继续推理，不是任务重放。

所有场景最终均满足：

```json
{"cleanupIntervention":[],"remainingWorkers":[],"remainingOwnedProcesses":[]}
```

采集过隔离 worker 的 FD/PIPE、进程组成员、可见后代及 owner/guardian 合成身份；最终检查的是已观察到的本次测试自有进程全部消失，不是宣称整台机器 FD 为零。测试原始配置逐字节保持不变。

测试缩短了时间阈值：通常断连 8 秒、终止宽限 1 秒；期限场景用断连 3 秒，寿命场景用最大年龄 6 秒，累计预算场景用 2 秒；空闲场景用空闲 2 秒加 drain 1 秒，并保持活跃状态 4–5 秒。硬 FD 场景采用警告/软/硬阈值 1/2/3。它们验证状态机与边界，不是长达一天的耐久测试。

## 回归测试及检查

- `cargo test --all-targets`：59 项通过，0 失败。
- `cargo fmt --check`：通过。
- `cargo clippy --all-targets -- -D warnings`：通过。
- `python3 -m unittest discover -s tests -p 'test_*.py'`：23 项通过。
- 官方协议核验：完整读取 `thread/loaded/list`，仅明确 `idle` 作为空闲；`activeFlags` 为空仍是活跃。待审批和待用户输入已用实际官方 server request 补足验证，不只是 schema 推断。

## 迭代中确实失败、最终已转绿的边界

旧入口不能拒绝同身份二连接，断连会产生不同 worker。首轮新入口另外暴露了 macOS accepted socket 继承非阻塞标志导致 EAGAIN、stdout 缺少逐块刷新导致初始化挂起。交叉审查及独立故障注入又复现了启动期间 owner 死亡、server 先死亡后的已脱组子进程，以及 guardian 单独死亡的清理缺口。全部对应最终场景均重新运行通过，没有只依据代码阅读宣布修复。

## 明确未被本轮验收覆盖的边界

1. 没有修改或重启已安装 managed SSH 通道，未实际切断物理网络，也未操作 Desktop 的恢复界面。连接故障由入口管道关闭模拟；真实网络黑洞、sshd 半开连接检测、系统休眠、路由切换和 Desktop 自动重订阅时延仍属于部署验收。
2. 没有调用真实模型，也没有真实 CUA、MCP 或浏览器业务操作；本轮不能保证外部服务的可用性或业务动作的恰好一次语义。
3. 无法保证 owner 与 guardian 同时不可恢复死亡、整机掉电后原内存 turn 继续运行。租约耗尽或硬限制触发后，设计会终止旧运行时，并非保证任何情况下无损续跑。
4. 脱组后代回收实测覆盖了运行中已观察并登记的进程。没有宣称能捕获发生于采样间隔内、从未被观察到的恶意瞬时 fork/setsid/退出逃逸；也不是操作系统级恶意代码隔离证明。
5. 安装、卸载及隐私检查通过的是测试套件中的隔离用例，不是对真实主机执行安装/卸载。

因此，可认为本轮约定的有界会话、单身份单活动连接、同 turn 重附着、交互恢复及已验证故障下资源收敛具备开发验收依据；实际部署之后仍应在用户授权窗口独立验证真实 managed SSH 与 Desktop 的完整路径。
