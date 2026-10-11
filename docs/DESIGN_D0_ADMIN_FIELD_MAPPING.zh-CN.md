# D0：Admin Shell 与 Overview 真实字段映射

> 状态：**已冻结首批 V2 视觉重构范围。** 本文只覆盖 Admin Shell 和 Overview，
> 不声明整个控制面、邮件端或 Agent Client Surface 已完成 D0。后续 D3 实现必须以本文
> 和实际 API 契约共同为准；没有列出的字段默认不渲染。

## 1. 本切片的目标

将 Admin 首屏从两个同权的数值面板收敛为一个**受限待决事实**：只根据已经成功读取的
开放案件和有效 Legal Hold，显示一个真实的下一步。它不是企业健康度仪表盘，也不读取、
计算或推断未提供的数据。

本切片不改后端 API、权限、URL、浏览器历史、会话、审计语义或路由模型。导航继续是
本地 `View` 状态；敏感资源 ID 不进入 URL、localStorage 或预取队列。

## 2. 已确认的事实与可见边界

| 资源 | 现有读取 | 可显示 | 主动作 | 严禁显示 |
| --- | --- | --- | --- | --- |
| Shell 组织上下文 | 当前会话/配置已有 `organization_id`；后端存在组织详情接口，但前端尚未有严格 `getOrganization` DTO | 截短、可复制的组织 UUID | 分组导航、语言切换、退出 | 操作者姓名、邮箱、角色、权限集合、组织名称/slug、健康度 |
| Open cases | `listCases({ status: "open", limit: 100 })` 返回 `items` 与 `has_more` | 当前成功响应中已读取的数量；`has_more=true` 时只写“至少 N” | 进入 Cases | 精确总数、风险评分、SLA、案件优先级、案件理由、邮件量、Agent 数 |
| Active legal holds | `listHolds({ status: "active", limit: 100 })` 返回 `items` 与 `has_more` | 当前成功响应中已读取的数量；`has_more=true` 时只写“至少 N” | 进入 Holds | 到期时间、保留期限、Archive 内容、审批结论、风险等级 |
| 双空态 | 两个列表都成功、`items=[]` 且 `has_more=false` | “当前可读取范围内没有待处理的案件或有效 Hold” | 无 | “一切正常”“零风险”“建议创建案件”等补造内容 |

### 2.1 已知但本切片不接入的组织名称

后端已有 `GET /organizations/{id}`，可返回 `display_name`、`slug`、`organization_id`，但
Admin client 尚未具备严格解析、403/503 回退和相应测试。因此 D3 不读取它，也不根据
配置猜测组织名称。未来若接入，必须单独交付最小 DTO、同源 no-store 策略、成功/403/503/
畸形响应测试；失败时继续只显示短 UUID。

## 3. Overview 决策算法（Fail Closed）

```text
if cases is ready and cases count > 0:
    primary action = “查看开放案件” → 本地 navigate("cases")
else if cases is ready and cases count == 0
        and holds is ready and holds count > 0:
    primary action = “查看有效 Legal Hold” → 本地 navigate("holds")
else if cases is ready and cases count == 0
        and holds is ready and holds count == 0:
    no primary action; render the verified empty state
else:
    no global priority claim; keep every source's loading / denied / error fact isolated
```

`ready && count == 0` 的含义仅限当前已成功读取的页。`403`、网络/服务器错误、加载中、
已过期请求、取消请求和未确认的 `has_more` 都不是零，绝不能被显示为零、空态或全局优先级。

CTA 只改变当前 React 中的本地 `View`；点击不得读取 Case/Hold 详情、不得触发额外
`listCases` / `listHolds`、不得写入 URL、History 或存储。

## 4. 审计、缓存和读取频率

Overview 的两个现有列表成功读取会分别产生
`compliance.case.queried` / `compliance.legal_hold.queried` 审计与 Outbox 证据；读取和
证据写入在服务端原子失败关闭。因此：

- 首屏只保留既有的一次并行读取；不得加 hover 预取、轮询、定时刷新、重复 retry 或为
  视觉动画额外发请求。
- 已有代次隔离继续防止陈旧响应覆盖界面；D3 不得降低它。
- 403、404 和错误状态使用 `AdminErrorNotice` 的受控码/Request ID；不渲染服务端
  message、trace、details、URL 或堆栈。
- `ControlPlaneClient.request()` 当前没有统一的显式 `cache: "no-store"`，而 Cases/Holds
  成功响应也未作为本切片的 no-store 证据。这个缓存收敛是独立安全评审项，不能通过增加
  Overview 读取面来“顺便修复”。

## 5. Shell 版式冻结

| 宽度 | 目标 | 当前实施要求 |
| --- | --- | --- |
| ≥1280px | 232–248px 全文字 rail、四组导航、可见标签 | 已实现 236px 分组 rail、原创 SVG 和 `aria-current` |
| 1024–1279px | 64px 窄 rail，必须有可访问名称和显式展开机制 | 已实现 72px 窄 rail 与具备可访问名称的 44px panel 控件；完整导航在显式抽屉中 |
| ≤1023px | 当前页标题 + 分组抽屉 | 已将抽屉切点迁移到 1023px；继续复用共享 Dialog 的焦点/背景隔离 |
| 所有宽度 | 主内容最大 1440px，保留足够左右留白 | 已将 `.workspace` 从 1580px 收敛为 1440px；浏览器断点测试覆盖上限 |

窄 rail 不会静默删除导航能力：它只保留具名的显式展开入口，完整分组导航与会话控制均在
抽屉中可达。

## 6. D3 必过的测试矩阵

1. Overview 单元测试：Cases 优先、Holds 次优先、双空、任意 loading/denied/error 不产生
   伪优先级、`has_more` 下界文案、CTA 无额外请求且只本地导航。
2. 现有独立 403/503/Retry 测试保持；错误不能降级为空态。
3. Shell 行为测试：1280 全 rail、1279/1024 窄 rail、1023/768/390 抽屉；键盘打开、
   Escape、背景关闭、路由选择后的焦点/读屏策略均有断言。
4. 浏览器：1440、1280、1024、768、390px 的中英文截图与 Axe；以 `.app-shell` 而不只是
   `.workspace` 执行无障碍扫描；检查页面无横向溢出和小屏 44×44px 控件。
5. 视觉快照不能用全量自动接受方式更新。V2 色板已从旧紫色转向 jade，必须逐张人工审图。

2026-08-24 本切片的单元、键盘/焦点、断点、Axe、无横向溢出和逐组审图门禁均已完成；完整
Playwright 套件为 94/94 通过。该结果只证明本地内存 BFF 夹具和生产构建工件，不替代真实
控制面、数据库、身份提供方或部署环境验收。

## 7. 与 V2 设计规格的关系

本文是 [`DESIGN_DEVELOPMENT_SPEC_V2.zh-CN.md`](DESIGN_DEVELOPMENT_SPEC_V2.zh-CN.md) 中
**D0 的首批可实现交付物**。D1 只负责共享 token 基础；D3 才实施 Shell 与 Overview。
邮件端、审计导出、Agent Dossier、其他治理页面仍需各自的字段映射，不能借本表扩大可见性。
