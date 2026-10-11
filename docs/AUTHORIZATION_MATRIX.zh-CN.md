# CopilotMail V2 授权矩阵

> 状态：P2 基线，2026-07-26。本文描述已经实现的租户 RBAC 目录、邮箱资源授权与审批收窄层。

## 判定公式

一次邮箱操作只有在以下条件全部成立时才允许：

`Active Principal ∩ 同租户 ∩ RBAC(mailbox.use) ∩ 邮箱资源授权 ∩ 操作约束`

任一信息缺失、未知或不匹配都拒绝。资源不存在、跨租户、跨邮箱、委托过期、委托撤销和 scope 不足，对 API 调用方应表现为相同的“资源不可见”，不得形成邮箱枚举接口。

当前 `GET /api/v2/mailboxes/{mailbox_id}` 已按此规则返回安全邮箱投影；它不会返回邮件正文、成员列表或委托详情。

邮箱管理 API：

- `POST /api/v2/organizations/{organization_id}/mailboxes` 要求实时 `mail.admin`，认证 Actor 成为该邮箱 Owner；
- `POST /api/v2/mailboxes/{mailbox_id}/delegations` 要求实时 `authorization.manage`，且 Actor 必须是该邮箱当前有效 Owner；
- `DELETE /api/v2/mailbox-delegations/{delegation_id}` 使用同样的权限与 Owner 约束；
- 所有写入都以认证 Actor 记录审计，不接受正文中的 actor、tenant 或 grantor 覆盖。

Human 与 Agent 使用同一授权内核。二者的登录方式、交互界面和批处理协议可以不同，但安全语义不得分叉。

## 邮箱资源能力

| 能力 | 含义 | 可委托 | 附加条件 |
|---|---|---:|---|
| `mailbox.read` | 读取该邮箱可见数据 | 是 | 后续正文读取仍可被合规策略收窄 |
| `mailbox.send` | 以自身正常身份从该邮箱发送 | 是 | 不接受 `on_behalf_of` |
| `mailbox.send_as` | 代表明确的授权人发送 | 是 | `on_behalf_of` 必须精确等于委托人 |
| `mailbox.manage_delegations` | 管理该邮箱委托 | 否 | 仅邮箱 Owner；企业治理操作仍受管理权限约束 |

## 成员关系

| Mailbox Role | Read | Send | Send As | Manage Delegations |
|---|---:|---:|---:|---:|
| Owner | 允许 | 允许 | 允许（自身） | 允许 |
| Member | 允许 | 允许 | 拒绝 | 拒绝 |
| Delegate（旧标签） | 拒绝 | 拒绝 | 拒绝 | 拒绝 |

`Delegate` 标签不携带隐式权限。委托访问必须有单独的 `delegations` 记录、明确 scope、开始时间、结束时间、委托人、受托人、邮箱和原因。

## 委托规则

- 委托人与受托人必须是不同 Principal。
- 邮箱、委托人、受托人必须属于同一 Organization，由数据库复合外键强制。
- 受托 Principal 必须处于 Active。
- 单次有效期最长 365 天；撤销后立即失效。
- 相同“委托人 + 受托人 + 邮箱”同一时刻只允许一条未撤销记录。
- 未知数据库 role/scope 按拒绝处理并记录错误。
- 授权查询只装载判定时刻有效且未撤销的记录。

## RBAC 上限

| RBAC 权限 | 作用 | 明确不代表 |
|---|---|---|
| `mailbox.use` | 允许进入邮箱资源判定 | 对租户全部邮箱的访问 |
| `agent.mail.read` | Agent 读取 Feed 精确引用的有界纯文本正文 | 任意 JMAP ID、HTML、附件内容或 Human 状态 |
| `mail.admin` | 邮件基础设施与配置管理 | 阅读邮件正文 |
| `authorization.manage` | 角色、策略和授权治理 | 绕过邮箱 Owner 或审批 |
| `legal_hold.manage` | 进入合规案件与 Legal Hold 审批执行流程 | 无案件、无精确审批或无远端回读的保全变更 |
| `retention.manage` | 创建、版本化并进入保留策略激活流程 | 绕过双人审批、Legal Hold 或直接删除数据 |
| `content.review` | 进入合规正文审查流程 | 无案件、无审批的任意正文读取 |
| `break_glass.invoke` | 发起紧急访问流程 | 自动取得资源权限 |
| `session.revoke_others` | 经双人审批撤销指定 Principal 的本控制面浏览器会话 | `identity.manage`、`security.admin` 或撤销操作者当前会话 |

## 内置角色目录

每个新租户在 Bootstrap 事务中创建完整九角色目录；Migration 0035 为既有租户补齐
同一目录。角色 key 与初始权限由 Rust 领域模型统一定义，不能依赖前端硬编码。

| 内置角色 | 初始权限 | 安全边界 |
|---|---|---|
| Organization Owner | 除 `content.review`、`break_glass.invoke` 外的租户治理权限，含 `session.revoke_others` | 高风险动作仍需精确审批；不自动读取正文；批量撤销他人会话不包含在 `identity.manage` 或 `security.admin` 中 |
| Mail Administrator | `organization.read`、`mail.admin`、`mailbox.use` | `mailbox.use` 仍需成员关系或委托；不能读取任意正文 |
| Security Administrator | `organization.read`、`security.admin`、`audit.read` | 不含角色管理、正文审查或紧急访问 |
| Compliance Auditor | `organization.read`、`audit.read`、`audit.export` | 保持只读；导出仍需双人审批和短期凭证 |
| Content Reviewer | `organization.read`、`content.review` | 仍必须绑定案件、审批和精确资源 |
| Helpdesk | `organization.read`、`identity.manage` | 当前身份权限粒度仍需在角色管理 API 中进一步收窄 |
| Agent Operator | `organization.read`、`agent.operate`、`agent.mail.read`、`mailbox.use` | 正文读取仍需 Token 显式 Scope、Feed Message UUID 与邮箱 Read ABAC |
| Employee | `organization.read`、`mailbox.use` | 只能使用被明确分配的邮箱 |
| Break Glass Operator | `organization.read`、`break_glass.invoke` | 只可发起紧急流程，不自动取得正文或资源权限 |

角色治理已形成受保护闭环：只有具备实时 `authorization.manage` Scope/RBAC 的
Active Human 可以查看目录与分配记录、授予或撤销角色。授予必须设置不超过 365 天的
有效期，并消费至少两人通过、由当前操作人申请的
`authorization.privilege_elevation` 精确审批；审批资源 ID 由目标 Principal、角色和
毫秒级失效时间共同构成，不能跨目标、角色或期限复用。读取、授予和撤销分别提交
不可变 Audit + Outbox，最后一个有效 Organization Owner 不能被撤销。非 Human
主体只能获得 Agent Operator，避免通过通用角色分配扩大机器权限。

## 审批上下文

审批是收窄层，不是放权捷径。当前审批上下文已包含不可变策略版本、请求人、审批步骤、职责分离、有效窗口、资源范围、用途说明和可选工单。即使审批通过，调用仍必须通过 Principal、租户、RBAC 和邮箱资源授权。

当前支持 Send As、Send On Behalf Of、审计导出、保留策略变更、合规正文查看、权限提升、Legal Hold、Break-glass、批量删除和 `session.revoke_others` 的动作标识。角色授予、审计归档 Legal Hold、保留策略激活与他人浏览器会话撤销均已接入 Active Human、实时 RBAC/Scope、原申请人绑定和精确双人审批；其他尚未接入的入口不能宣称已受审批保护。他人会话撤销的执行权限只来自 `session.revoke_others`，默认只授予 Organization Owner。审批资源类型是 `identity.principal`，资源 ID 是目标 Principal。执行时不撤销操作者当前会话，同一审批不能再次使用。目标不存在、没有有效会话和路径租户与调用者租户不一致时，对外返回同一空成功结果，不提供会话计数。

## 审计动作

当前邮箱授权切片原子记录：

- `mailbox.created`
- `authorization.mailbox_delegation.granted`
- `authorization.mailbox_delegation.revoked`
- `authorization.role_assignment.granted`
- `authorization.role_assignment.revoked`
- `authorization.role_assignment.queried`

审计详情记录 mailbox、授权双方、scope、有效期和原因；不记录密钥、令牌或邮件正文。
