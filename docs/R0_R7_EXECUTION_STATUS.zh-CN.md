# R0–R7 执行记录

> 更新：2026-09-19。**下文为托管验证码需求撤回前的历史执行快照，不是当前待办清单。**
> 用户后续要求先完成代码、不部署，验证码由应用处理。原型与四项专属测试映射已撤下，历史
> 12 项领域测试/344 个 Rust 总数不能作为当前能力。现行范围见
> [当前代码开发范围](CODE_DEVELOPMENT_SCOPE.zh-CN.md)，原型见 [归档说明](archive/README.zh-CN.md)。

## 1. 当时的阶段快照（历史）

| 阶段 | 状态 | 本轮落地 | 未完成项 |
| --- | --- | --- | --- |
| R0 | 进行中 | 542 个非忽略/非凭据文件的实施前 SHA-256 清单；首批 7 项能力/函数测试映射；校验器与 CI | 全量函数清单、覆盖率工具与报告、冻结可发布候选、公司容量/用途/环境输入 |
| R1 | 首批拆分完成，阶段未完成 | 控制面 4,063 行测试移至同模块测试文件；283 行 Agent 检视 HTTP 适配独立；五类 PG query 实现分离 | 其余 HTTP/鉴权/Worker/PG transaction/ports 大模块与前端控制器仍需继续按职责拆分 |
| R2 | 未完成 | 增加内部 `ApplicationId` 类型作为领域引用 | Application 注册/生命周期、Service OAuth、scope/RBAC、凭据、治理路由、迁移均未实现 |
| R3 | 纯领域规则已实现，功能未完成 | 精确绑定、不可变策略边界、挑战状态机、一次性消费、重发、过期及收据模型；12 个规则测试 | CSPRNG/秘密存储 adapter、PG 事务/约束、幂等/配额、审计/Outbox、HTTP/正式 OpenAPI、真实并发测试 |
| R4 | 未实现 | 领域内将投递事实与验证码状态分开 | dispatch intent、Worker、精确邮件对账、退避/恢复和真实发送接线 |
| R5 | 局部修复 | 修复桌面 Compose 丢失共享样式与桌面 Back 控件误显示；增加独立几何断言 | 应用管理页面、其余 D5–D8 收尾、接入示例和双语功能文档 |
| R6 | 本地回归通过，真实验收未完成 | 344 Rust、411 Web unit、109 Playwright；构建、Clippy、现有静态门禁通过 | 36 个依赖场景未跑；真实企业 IdP/域名、PG/Object Lock、Docker、恢复/容量/故障验证缺失 |
| R7 | 未完成，不放行 | 新领域规则做过有界只读复核并修复发现项；发布模式主动拒绝不完整证据 | 全仓完整审查、覆盖率、独立最终复核、冻结候选与全部阻断项关闭 |

**R2–R5 仍有实际编码剩余，不是仅等待测试环境。** 当前正式 OpenAPI 不包含验证码路由，
没有应用登录入口、明文码调试入口、默认凭据或假发送结果。

## 2. 可追溯基线与测试矩阵

- [实施前源码清单](../quality/baselines/R0-2026-09-19.json)：HEAD 为原有 `fabf73e`，
  工作区摘要 `64bac2ea6f59300fdabe383c2bedcf20f655fc423ba6bc3c3f7532970de989e8`。
  清单不是备份、Git 提交或测试结果；不包含忽略文件、凭据文件及报告自身，不覆盖未纳入清单的秘密配置。
- `scripts/capture_quality_baseline.py` 拒绝覆盖既有报告，仅可输出到工作区 `quality/baselines`。
- [函数/能力测试映射](../quality/function-test-matrix.yaml) 使用 JSON 兼容 YAML；目前明确标为
  `inventory_scope=initial`，不能当作所有函数已经覆盖。
- `scripts/check_function_test_matrix.py --self-test` 校验模块/符号/测试引用、正常/边界/失败/
  权限/事务/并发类别，以及发布证据绑定；两组自测含多项拒绝场景。源码引用检查不替代编译、
  实际执行、覆盖分析或人工审查。
- 默认 CI 允许诚实记录在开发中的能力；tag 候选使用 `--release`，当前会正确拒绝初始清单、
  缺失实跑/复核证据，不得通过删除该门禁或手工伪造 `verified` 放行。

## 3. 内部验证码模型的边界

`crates/copilotmail-domain/src/verification/` 按 binding、policy、challenge、receipt 分文件：

- 业务 flow 使用受检查的 UUIDv4；绑定组织、应用、用途和带密钥的目标引用，不接受换绑。
- 默认有效期 300 秒，错误最多 5 次，显式重发最多 3 次、冷却 60 秒；重发不清零失败次数、
  不延长原始有效期，不因 delivered 自动变成 verified。
- 到期按传入的权威时间即时生效，不依赖后台清理；拒绝时间回退。
- 快照恢复检查不可变生命周期、计数、单调版本、终态、最后派发时间及策略版本引用。
- 验证收据从已验证挑战派生，精确绑定策略/flow/目标，并检查自身有效窗口。它不是登录 Token。
- 规则方法不访问数据库、HTTP、随机源或邮件服务；`VerificationMatch` 是未来受信加密校验器的
  内部输入，不可从 HTTP 反序列化。当前没有生成或真正比较验证码，也没有 SQL 层一次性消费保证。
- 12 个测试包含 15,625 组短操作序列、各维度换绑、五次失败、重发/到期/终态、溢出时间、坏快照
  和收据边界。它们不证明多进程/PG 并发或对外 API 安全。

有界复核后删除了基于计数不变量已不可达的版本溢出分支，增加独立收据恢复检查及 Locked/Cancelled
清理回归；这只是新模块审查，不是 R7 的全仓审查结果。

## 4. 重构与视觉修复

- `apps/copilotmail-control/src/lib.rs` 从约 17,662 行降至 13,319 行；测试仍在同一 Rust `tests`
  模块内，通过 `tests.rs` 保持私有访问和原测试名称。`agent_inspector_http.rs` 只搬迁原 HTTP 适配。
- `crates/copilotmail-postgres/src/queries/` 分离 Human mailbox/draft/sent、Mail domain、Role 查询；
  原入口从约 16,085 行降至 14,845 行。SQL 和 trait 行为没有重写，事务仍由原 store 所有。
- 这些只是第一步结构分离；内部共享依赖和原大 transaction trait 仍存在，不宣称架构债务已清零。
- Compose 重新消费现有 `.button`，不追加一套私有颜色；Back 控件恢复为窄屏显示。浏览器测试除
  截图外，独立验证桌面 Compose 至少 40px、共享样式类存在、Back 桌面隐藏/窄屏可见。
- 相关截图显式更新后已在不更新模式下复跑全部 109 用例。桌面收件箱与人工复核代表图已人工复核；
  R7 仍需要覆盖全部候选页面的最终视觉评审，不能将代表图抽查写成全量设计签字。

## 5. 2026-09-19 实跑记录

| 检查 | 结果与范围 |
| --- | --- |
| `cargo test --workspace --locked --quiet` | 344 passed，0 failed，36 ignored；Domain 55（含本轮 12），Application 116、Control 64 非忽略测试通过 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过 |
| Domain 独立规则测试 | 12/12；不连接 PG/邮件服务 |
| Web 类型检查、三个 workspace 受限 worker Vitest | 通过；17 + 207 + 187 = 411 |
| `npm run build` / Bundle | 通过；Mail JS 322,061 B，CSS 49,141 B；未放宽预算 |
| `npm run e2e:run` | 109/109；仅回环 Vite 和同源内存 BFF 夹具，103 张 PNG |
| 架构、审计、Compose、设计、i18n 静态门禁 | 通过；审计目录仍为 85 类，无新增验证码事件或迁移 |
| 函数映射自测 / 发布模式负向测试 | 自测通过；发布模式拒绝不完整清单（预期结果） |

Windows 链接器仍报告已有 AWS SDK 调试信息 LNK4209 警告；测试命令退出码为 0，Clippy 为 0。
未清理其他工作负载或以关闭安全检查处理该警告。

36 个 ignored 场景包含 30 个 PG integration、4 个 MinIO Object Lock 和 2 个需可丢弃 PG 的控制面
HTTP 场景。不能将“已编译、被忽略”记录为运行通过；也未产生所要求的覆盖率报告。

## 6. 当时的环境事实与安排（已被当前范围替代）

- 本机未发现 Docker 命令，未配置 `COPILOTMAIL_DATABASE_URL` 与 Object Lock 测试连接。
- 对指定 NixOS 仅执行了已知主机密钥校验下的免交互 SSH 只读预检，认证返回 Permission denied；
  没有执行远端命令、部署、migration、修改端口/服务或使用故障 WSL。
- 后续 R6 需要可用的安全 SSH/隔离 Docker 接入、专用测试 PG/Object Lock、企业 IdP 与批准域名/
  收件箱及容量/恢复目标；不能在共享生产应用上试错或以开发夹具代替。
- 代码侧下一段首先补 R2 的独立 Application 身份/授权与持久化，再将现有纯规则接入 R3 的完整
  PG 用例、R4 Worker 以及 R5 管理界面；不得先提供一个假成功的验证码端点。
