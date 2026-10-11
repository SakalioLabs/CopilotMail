# 代码质量收敛与审查记录

更新：2026-10-12。当前范围是代码开发与本地验证；**不部署、不连接生产数据库、不操作 WSL/NixOS**。
Human Mail、Agent Mail、企业管理和既有 API 保持；验证码由业务应用生成/验证，使用标准邮件协议发信。

## 1. 已修复的问题

| 问题 | 修复与证据 |
| --- | --- |
| Compose 同一渲染帧可重复保存/提交；未知结果确认可并发重试 | `useComposerSession` 用同步、按代次归属的锁控制入口；重试保留原幂等键。新增 4 项回归，包括旧操作不能释放新会话的锁、Reset 释放等待者；Hook 共 11 项通过。 |
| Retention 仅信任旧权限快照；授权前读取详情；池大小为 1 时嵌套查询可等待自己 | 权限、详情和审计使用同一事务连接；精确检查租户/资源/版本；12 项用例含 CAS、权限撤回、每个写入/审计/Outbox/提交失败点。 |
| Retention 激活使用调用者时间，锁等待后可能已过审批有效期 | 新迁移 0059 在锁后使用数据库时间，并在激活指针变更时重新验证审批；应用审计/Outbox 使用返回的持久化时间。已补隔离 PG 场景，尚未实跑。 |
| 审计导出重放绕过事务内当前权限检查；适配器错返 ID 缺少拦截 | 重放重新验证权限且不延长凭证；校验组织、Grant、Archive、申请人、审批和对象版本。3 个新增负例先失败、修复后通过；服务新增 9 项测试。 |
| 审计下载可用旧时间落账，返回证据未绑定持久化时刻 | 新迁移 0060 使用锁后的数据库时间；下载 port 返回完整证据，服务拒绝换绑/过期证据，并以存储时间记录 Audit/Outbox。隔离 PG 测试加入倒填时间拒绝场景，尚未实跑。 |
| 审批申请缺少应用层基础/当前权限校验；撤回仅检查主体存活 | 申请与撤回检查对应 action 权限并事务内重新验证；保留 Human/Agent 两种申请人，审批决策仍仅 Human。CLI 策略维护必须绑定同租户、当前有管理权限的 Human Owner；新增 11 项纯规则/服务测试。SQL 决策写入和撤回还以数据库当前时间拒绝锁等待后过期的请求；补隔离并发场景，但未实跑。 |
| Agent 凭据用例缺少独立异常路径测试 | 新增 7 项服务测试（含逐写入失败循环），检查 owner 换绑、scope、到期边界、mTLS、轮换与撤销；敏感密钥不进入审计。 |
| 审计治理脚本漏扫嵌套模块、多行提交和私有函数；可借其他事务的证据通过 | 位置保留的词法扫描，按函数/事务变量/实际调用 helper 归属检查；14 项门禁自测。仅说明源码证据存在，不宣称完成控制流形式化证明。 |
| 新迁移的 Dollar Quote 拼写错误未被 Cargo 编译发现 | 修正 0059；增加 PostgreSQL 17 SQL/PLpgSQL 离线解析门禁与 4 项自测，全部 60 份迁移可解析。SQL 解析不是执行，也不校验真实表、权限、索引和锁行为。 |
| 仅检查旧表/索引不能证明新时间校验触发器已经迁移 | readiness 新增完整 migration ledger/checksum 匹配；缺失、失败、被改写或未知版本一律未就绪。深层 SQL 就绪检查移为独立文件且继续执行；2 项纯测试覆盖拒绝分支。 |
| 测试可能误连有业务数据的数据库 | PG integration 必须显式 opt-in，库名限定为 `copilotmail_test_*`；CI 仅允许回环专用库例外，拒绝 URL 查询/片段覆盖。默认只执行离线安全门禁。 |
| Vitest 开发服务器依赖存在已公告的文件读取漏洞 | 全部 workspace 升级并锁定 Vitest/coverage 4.1.11；`npm audit` 当前为 0。依据 [官方安全公告](https://github.com/vitest-dev/vitest/security/advisories/GHSA-82fw-gwwq-j7x9)。 |
| Rust 依赖存在旧 HTTP/TLS/DNS 与内存安全公告 | 根据 RustSec 实扫升级 AWS SDK/Smithy、Hickory、Rustls/WebPKI、h2、event-listener、lru、chacha20；禁用 S3 的旧 Hyper/Rustls connector，保留受维护的 HTTPS connector；改用 rustls-pki-types PEM 解析，移除停止维护的 rustls-pemfile。当前 `cargo audit --json` 为 0 漏洞、0 警告。 |
| RustCrypto RSA 私钥签名存在网络可观测时序风险 | JWT 改用 AWS-LC，审计签名改用 ring；新增 `copilotmail-crypto` 安全封装、签名字节兼容测试与加密边界门禁。Cargo.lock 已移除 RustCrypto `rsa` 依赖；RSA 密钥解析使用 AWS-LC 封装，测试密钥生成仅通过 `test-utils` 特性开放。 |

权限锁顺序为组织治理锁 → 主体/角色授予/角色权限 → 业务资源，锁后再次检查到期时间；不将
新授权、Token Scope 或非 Human 特权从旧快照中推导出来。只读辅助复核检查了审批/导出服务和
新 SQL 时间契约；它不等于全仓独立最终签字，真实并发行为仍须 PG 验证。

## 2. 结构与可测试性

- 控制面 HTTP 按资源拆到 `apps/copilotmail-control/src/http/`，路由/契约保持；入口从本次开始的
  13,319 行降至约 6,493 行，仍保留运行时装配、CLI 和共享边界函数。
- PostgreSQL 分为 `repositories/`（资源查询）和 `transactions/`（同一 Unit of Work 内的写入）；
  原大入口从 14,845 行降至 4,092 行。事务 port 转发与 schema readiness 单独存放，不复制数据库连接或拆散原子提交。
- 保留策略、审批、导出、Agent 身份测试各自独立模块；失败夹具只在测试代码中，业务不依赖测试框架。
- 大 port 和部分 SQL/装配函数仍有进一步拆分价值。按文件搬迁不代表所有内部耦合都已消除。

## 3. 验证与覆盖口径

本轮最终命令结果见 [本地验证记录](../quality/reports/local-validation.json)。失败的中间编译/测试不计作通过，
修复后重跑；Windows 调试信息问题通过独立 target 与禁用增量调试缓存处理，不删除断言、不跳过测试。

- [Rust 函数清单](../quality/reports/rust-function-inventory.json)：显式源码函数/行数/哈希，不展开宏或匿名闭包。
- [TypeScript 函数清单](../quality/reports/typescript-function-inventory.json)：编译器 AST，包括箭头函数/方法；不等于测试覆盖。
- [TypeScript 测试声明](../quality/reports/typescript-test-inventory.json)：实际 AST 中的字面量测试标题，不凭注释推断。
- [审查前 Rust 覆盖率](../quality/reports/rust-coverage-before-quality-pass.json)：单独保留，不用新结果覆盖历史。
- [本轮 Rust 覆盖率](../quality/reports/rust-coverage.json)：与函数清单分开记录；不包含 ignored 的依赖场景。
- [Rust 依赖安全扫描](../quality/reports/rust-dependency-audit.json)：保存数据库版本、锁文件哈希和无忽略项的全绿结果。
- [Web 覆盖率](../quality/reports/web-coverage.json)：合并三套 workspace，避免漏记共享组件在应用中的实际执行。
- [能力/测试矩阵](../quality/function-test-matrix.yaml)：区分 passed / not_run；发布模式同时验证来源文件哈希，旧证据不能继续放行已修改代码。

矩阵现覆盖 105 项高风险/基础质量能力，仍不是完整函数级签字。前端本轮实测 416 项单测、
109 项回环浏览器回归通过；合并行覆盖率 89.12%、函数覆盖率 86.92%，不能称为全覆盖。

Rust 全工作区最终 389 passed、0 failed、43 ignored；LLVM 串行重跑也成功输出报告。
当前清单有 181 个 Rust 源文件、1,930 个显式函数，以及 54 个 TypeScript 源文件、1,309 个
AST 函数；宏、测试生成与平台差异使清单计数不能直接作为 LLVM 覆盖率的分母。

覆盖率里的执行命中不是断言充分性。Rust LLVM 报告包含 inline test harness/编译器生成函数，
当前未启用分支覆盖；不能把 `branches.count=0` 表述为“0% 已覆盖”，也不能把拆文件前后比例直接
当作等口径改善。原始报告置于被忽略的 `quality/coverage/`，摘要保留输入哈希与来源。

## 4. 不得冒充已完成的代码质量项

- 能力级测试矩阵清单已为 `complete`，但审查与发布签字仍未完成；词法/AST 清单不能自动改成 `verified`，也不是所有函数断言充分性的证明。
- Session/凭据撤销已通过 `begin_authorized` 统一绑定到主要请求写事务；PostgreSQL 在事务开始时锁定会话、Agent 凭证、SCIM actor，并在提交前按数据库时钟重新验证。后台 lease/reconcile 等无请求主体的内部作业继续使用专用事务入口。
- 完整函数级断言覆盖、全仓独立最终复核与可发布候选冻结尚未签字，不能宣布 R7/Enterprise GA 完成。

## 5. 最后执行的运行态验收

按用户要求统一延后：隔离 PostgreSQL 的迁移/触发器/并发/回滚/单连接验证、MinIO Object Lock、
Docker 镜像与容器运行、企业 IdP、真实 SMTP/IMAP 客户端、公网域名/DNS/邮件收发、压力与恢复演练。
不使用生产数据库代替一次性测试库，不将 ignored 或“CI 已配置”写成实跑通过。

PG 测试只允许经确认的一次性数据库，同时设置 `COPILOTMAIL_ALLOW_ISOLATED_DB_TESTS=1` 与
`COPILOTMAIL_DATABASE_URL`，数据库名为 `copilotmail_test_<suffix>`；URI 不允许 query/fragment。
测试会迁移并写入数据，这不是只读验收。
