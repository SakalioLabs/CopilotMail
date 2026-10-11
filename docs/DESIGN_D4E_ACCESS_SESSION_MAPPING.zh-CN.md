# D4E：Access / Session Trust Gate 真实字段映射

> 状态：**已实现（2026-08-24）。** D4E 收敛 Human Mail 在进入工作台前，以及
> 已打开工作台被安全复核锁定时的真实界面。它不改变 Cookie 会话、邮箱目录、授权、草稿或
> 提交状态机，也不证明 IdP、Docker、NixOS、真实邮件系统或生产环境已经验收。

## 1. 目标与范围

D4A–D4D 已分别收敛工作台 Shell、Queue/Reading、Drafts/Sent 与 Compose。D4E 补齐其中
尚未单独映射的准入边界：

1. 正在检查 Human Mail Session；
2. 未登录或无法开始登录；
3. 已认证但仍在解析、分页或读取授权邮箱目录；
4. 已认证但当前没有授权邮箱或目录读取失败；
5. 已打开工作台时的会话复核中与暂时失败。

每个状态只回答“浏览器现在是否已经拿到可用、已授权的 Mail Session 与邮箱范围”，不把
认证、邮箱目录或暂时网络错误画成身份、权限、投递或邮件健康度结论。

## 2. 已确认字段与披露边界

| 当前条件 | 可显示的真实事实 | 唯一允许操作 | 禁止显示或推断 |
| --- | --- | --- | --- |
| `auth.kind === "checking"` | `auth.checking` | 无 | 已登录、已失效、正在同步邮件、组织/账户资料 |
| `auth.kind === "signed-out"` | 产品名、已有登录说明 | 既有企业账号登录 | 账号、组织、IdP、设备或登录风险信息 |
| `auth.kind === "error"` | 已本地化的 `messageKey` | 同一登录入口 | 原始服务端错误、Cookie、Token、Trace、堆栈 |
| 无当前邮箱且目录加载中 | `mailbox.directoryLoading` | 退出 | 没有权限、没有邮箱或目录总量 |
| 无当前邮箱且仍有下一页 | `mailbox.directoryNeedsMore` | 已有“加载更多邮箱” | 自动无限分页、Cursor、页数、总数 |
| 初始目录读取失败 | `error.mailboxDirectoryUnavailable` | 已有“重试读取”、退出 | 租户、RBAC、后端失败细节或“会话无效”结论 |
| 已确认目录为空 | `error.mailboxDirectoryEmpty` | 退出 | 虚构邮箱、写入、重试登录或管理员联系方式以外的承诺 |
| 已打开工作台且 `sessionRefreshing` | `auth.checking` | 无 | 继续可操作、会话已经过期或写入安全完成 |
| 已打开工作台且 `sessionVerificationFailed` | `error.sessionUnavailable` | 已有重试、退出 | 自动重试、会话已失效的确定性断言、恢复写入 |

`HumanSessionResponse` 的 `principal_id`、`organization_id`、`client` 与 `expires_at` 只用于
控制器绑定和过期复核；D4E 不得把它们渲染为可见身份资料。`MailboxDirectoryPage.next_cursor`、
`mailbox_id`、目录页数、邮箱 Owner、Agent 运行/在线/策略也不得显示或推断。

## 3. 素材与视觉规则

- 已登记的 `mail-agent-routing-v1.webp` 仅用于未登录/正在建立登录入口的装饰性双栏主视觉；
  登录容器保持 `aria-hidden` 与空 Alt。
- 已认证后的目录加载、目录为空、目录失败和会话复核失败**不得**复用登录主视觉，也不新增
  Image 2 位图。它们是窄、事实优先的准入面或非模态安全提示，而不是营销式登录页。
- 会话复核提示固定高于被 `inert` / `aria-busy` 锁住的工作台；它不建立第二个 Dialog、
  焦点陷阱或背景遮罩。复核中只以单一 `role=status` 提示，失败才以单一 `role=alert` 提示
  并接收既有程序化焦点。
- 不使用渐变、玻璃效果、伪进度、骨架、账号头像、未返回的邮箱、假成功、假的“安全”徽章
  或无意义色块。动作只使用已有登录、加载更多、目录重试、会话重试和退出回调。

## 4. 响应式冻结

| 宽度 | D4E 规则 |
| --- | --- |
| `≥901px` | 登录入口可保留受登记素材约束的双栏；目录/会话事实面宽度受控，不伪装为登录卡。 |
| `561–900px` | 单列；登录主视觉隐藏；提示与动作自然换行。 |
| `≤560px` | 占满安全可用视口；登录、加载更多、重试和退出均至少 44×44px；无横向滚动。 |
| 会话复核提示 | 工作台之上、非模态；不遮盖真实下一步、不抢占焦点，失败时才使用既有 alert 焦点。 |

## 5. 不变量

1. 不新增 API、DTO、权限探测、URL/query/fragment、浏览器存储、缓存、轮询、预取或自动重试。
2. 不修改 `loadSession()` 的同源 Cookie、`cache: "no-store"`、无 Token 读写与登录跳转规则。
3. 无 `activeMailbox` 时，仍不得读取 Inbox、详情、草稿、已发送或提交接口。
4. 保留目录精确分页、初始十页上限、邮箱绑定、Abort / generation 隔离和主动登录/退出流程。
5. 会话复核期间继续锁住工作台、快捷键、写入和轮询；失败后只允许显式重试或退出。
6. Retry 成功后恢复现有工作区和本地 Compose 内容；D4E 不能把它写成一笔业务写入已经完成。

## 6. 实施边界与验收

`MailAccessSurface` 与 `SessionVerificationNotice` 已从 `App.tsx` 拆出：两者仅接收已经本地化的
文本、已决定的状态和既有回调；不导入 `auth.ts`、`api.ts`，也不拥有请求、会话、目录或持久化
状态。`App.tsx` 仍唯一拥有认证、目录加载、复核、重试、退出和焦点语义；工作台 shell 不再另行
渲染隐藏的 session-refresh 文案。

实现与证据如下：

1. 登录、登录错误和检查中使用 `MailAccessSurface kind="login"`；已认证的目录加载、目录为空、
   目录失败与显式继续分页使用无 Hero 的 `kind="directory"`，不会读取邮件、草稿或提交资源。
2. `SessionVerificationNotice` 在复核中只提供一个可见 `role=status`；失败时为可聚焦的非模态
   `role=alert`，复用已有重试/退出回调，`mail-shell` 的 `inert` / `aria-busy` 与恢复焦点契约
   保持不变。
3. `App` 与认证回归继续覆盖目录初始十页上限、显式继续分页、无活动邮箱时不读取 Mail 资源、
   会话锁定和重试恢复。新增 `mail-access-session.spec.ts` 以同源内存 BFF 夹具覆盖英文/简体中文
   登录、已确认目录为空、目录暂不可用、会话复核失败的 1440/901/390px 截图、Axe A/AA、无横向
   溢出和 44px 几何。
4. 受限 worker 的 Vitest 共 411 条（设计系统 17、Admin 207、Mail 187），`npm run check`、
   `npm run build`、`npm run e2e:check` 与 `npm run e2e:run`（109/109）均通过；D4E 增加 15 张
   Mail PNG，当前基线为 103 张（Admin 40、Mail 63），已人工审图。

以上均是本机 Vite 产物与同源固定夹具的界面/回归证据，不证明真实 IdP、RBAC、数据库、Docker、
NixOS 或生产邮件依赖已经验收。
