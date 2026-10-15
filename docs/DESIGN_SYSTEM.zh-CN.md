# CopilotMail 设计系统开发规范

> 状态：实施中的设计契约。本文是 [`DESIGN.md`](../DESIGN.md) 的中文开发说明，
> 用于约束 Web 邮件端、管理端和后续 Agent 观察/控制界面的设计与验收。
> 若本文与 `DESIGN.md` 的视觉原则冲突，以 `DESIGN.md` 为准；人与 Agent 的产品
> 行为以 [人类与 Agent 交互规范](HUMAN_AGENT_UX_SPEC.zh-CN.md) 为准；位图资产
> 以 [Image 2 素材清单](IMAGE2_ASSET_MANIFEST.zh-CN.md) 为准。

## 1. 唯一视觉方向

CopilotMail 只采用一种产品视觉方向：**Measured Agent Workbench / 有界智能体工作台**。
它是一种紧凑、高信息密度、证据优先的工作台语言，目标是让操作者快速理解“当前是谁、
正在操作哪个范围、状态是否可信、下一步能做什么”。Human Mail 的键盘优先、快速
完成任务的工作方式可称为 “Superhuman Operational”，但这只是交互姿态，不是第二套
视觉语言，也不是对任何第三方产品的复制。

- 邮件客户端的节奏、层级和效率只作交互参考，不复制任何第三方页面、Logo、文案、
  标志性配色、布局或动效。
- Agent 的视觉语言使用原创的暖纸、石墨线稿、受控线性路由和少量 jade 行动信号；jade
  只表达焦点、已授权操作或事实性 Agent 关联，不能代替身份或成功状态。它可以借鉴
  当代 Agent 产品的克制感，但不得复刻第三方吉祥物、机器人脸、品牌轮廓或图标。
- 默认是浅色工作台。深色主题只能从同一组 token 推导，不能引入第二套视觉语言，
  也不能以渐变、霓虹、光晕或大面积彩色块替代信息层级。

这不是营销落地页风格。正常工作区应主要呈现暖白、白、墨黑和灰；色彩只承担焦点、
Agent 语义或明确的状态含义。

## 2. 设计原则与禁止项

### 2.1 必须成立的原则

1. **操作优先于装饰。** 每个元素必须解释身份、范围、状态、风险或下一步操作。
2. **Human 与 Agent 均为第一公民。** 身份通过图形、形状和文字共同表达，不能只靠
   颜色，也不得从邮箱地址猜测 Agent Owner、权限或委托关系。
3. **界面必须忠实于后端。** 没有 API 字段、授权结果或审计证据的状态不能被“设计
   出来”。提交已受理、投递完成、人工复核、策略阻断和 Agent 归因必须彼此区分。
4. **每个视图只有一个主操作。** 次要工作进入按需面板、溢出菜单或命令面板；不把
   所有未来功能固定堆在工具栏上。
5. **键盘优先、鼠标完整。** 现有操作必须同时可用键盘和指针完成；移动端主要触点
   至少 44 × 44px。

### 2.2 生产界面禁止项

以下内容不得以“后续再接 API”为由进入生产构建：

- Lorem ipsum、伪造姓名/头像/指标/时间线、永久骨架屏、死按钮、无意义空卡片、
  无动作的彩色背景块和装饰性 blob；
- 用一张通用插画同时代表搜索无结果、域名未接入、发送失败和无权限；
- 由前端推断的 Agent 来源、Owner、Scope、策略判定或投递成功；
- 无文本名称的图标按钮、仅 hover 可见的关键信息、只用颜色传递的身份或错误；
- 第三方商标、Logo、界面截屏、生成式文字图形、水印、二维码和未经登记的位图。

加载、空、错误、只读、未知提交和人工接管都是产品状态，不是可被空白面板掩盖的
视觉间隙。每个状态都要说明真实触发条件，并给出后端确实支持的下一步。

## 3. Token、排版与版式

共享 token 定义在 `web/design-system/src/styles.css` 的 token layer。功能 CSS 只能引用
语义 token；除了 token 定义层，不得新增原始色值。

| 语义 | Token | 初始值 | 使用边界 |
| --- | --- | --- | --- |
| 页面暖纸 | `--cm-canvas` | `#F8F8F6` | 应用背景 |
| 内容表面 | `--cm-surface` | `#FFFFFF` | 阅读、写信、数据区 |
| 主墨色 | `--cm-ink` | `#17202A` | 主文字与主操作 |
| 辅助文字 | `--cm-muted` | `#667085` | 次级元数据 |
| 结构线 | `--cm-line` | `#E1E6E3` | 分栏与分组边界 |
| 控件边界 | `--cm-control-border` | `#7A8795` | 表单与次级操作的可感知轮廓，白底至少 3:1 |
| Agent / 行动信号 | `--cm-agent` | `#0F766E` | 选中、焦点、已授权行动、事实性 Agent 关联 |
| 成功 | `--cm-positive` | `#18794E` | 完成或投递成功 |
| 警告 | `--cm-warning` | `#996300` | 等待、复核、风险提示 |
| 严重 | `--cm-critical` | `#B42318` | 失败、破坏性操作 |
| 中性信息 | `--cm-info` | `#245FC7` | 系统说明 |

- 使用共享 `--cm-font-sans`（含 Inter、PingFang SC、Microsoft YaHei、Noto Sans CJK SC
  和系统回退）；等宽 `--cm-font-mono` 仅用于快捷键、ID、哈希、Trace 和审计证据，不能
  从页面 CSS 自建另一套字体栈。
- 使用 4px 微单位和 8px 主节奏：`--cm-space-1…12`、语义字号/行高、共享 44px 触点、
  圆角、motion 与 z-index token 都只在设计系统定义。D1 先冻结基础；页面级间距和字号
  迁移随 D3/D4/D8 分批完成，不能机械全局替换。
- 大面积背景不使用状态色。状态色只能与文本、图标、边线、标签等紧凑信号共同出现。
- 宽屏邮件工作台采用“作用域 rail → 消息队列 → 阅读/写信区”的三栏布局；队列列宽使用
  有界的连续伸缩轨道，1024px 仍必须给阅读区保留可读长主题、地址元数据、正文和附件的
  宽度，不能把三栏硬压成逐词折行的窄缝；真实存在的 Agent 证据才允许打开第四个、默认
  收起的上下文检查器。
- 草稿和已发送若当前接口没有可展示的详情数据，使用宽列表而非保留一个空白详情栏。
  窄屏改为明确的列表—详情栈，不能把桌面三栏强行压缩。

## 4. Human / Agent 身份与状态表达

### 4.1 身份图形语法

| 种类 | 图形语法 | 必须出现的文本 |
| --- | --- | --- |
| Human | 圆形个人/轮廓图标 | `Human` 或人员名称 |
| Agent | 圆角邮件核心与执行轨道 | `Agent`，有真实字段时附 Owner/Scope |
| Shared | 成对收件箱/托盘图标 | `Shared` |
| Group | 路由节点邮箱图标 | `Group` |
| System | 受边界约束的系统核心图标 | `System` |
| 委托操作 | 操作者加方向路径 | `on behalf of …` 或本地化同义文本 |

共享 `IdentityBadge` 以原创 SVG 与对应形状承载上述身份语法；调用方必须传入当前界面
语言下、始终可见的 `label`，组件不会补出隐藏的英文默认值，也不会伪造头像、Owner、
Scope、委托或状态。角色与审计的持久化 `service` Actor 使用独立的 `service` 身份种类；
它可采用受边界系统核心图形作为视觉语法，但可见文本必须始终为 `Service`/`服务`，
不得改称 `System`/`系统`。

Agent 的网页体验不是让它“像人一样点按钮”。Agent 的一等公民入口是稳定、低歧义、
可审计的 API、SDK 与事件；人类界面负责呈现 Agent 邮箱、Owner、授权范围、运行状态
和人工接管。只有后端提供 `actor`、`on_behalf_of`、Owner、Scope、策略决定、执行 ID
或审计引用时，才显示相应信息。

`StatusIndicator` 只接收 API 已给出的事实状态；它必须同时有语义图形、紧凑形状/色调
和可见的本地化文字。异步状态变更才可启用读屏播报，不能把推断的运行、投递、权限或
审批结果塞入该组件。Agent 的状态在有数据时还应包含原因和下一步；不能仅用旋转动画、
脉冲光点或颜色变化表达“正在运行”。任何不可见、无权限或未知状态均应明确说明边界，
而不是以默认成功状态代替。

### 4.2 真实状态的最小要求

| 场景 | 页面必须呈现 | 不得伪造 |
| --- | --- | --- |
| 当前邮箱（身份卡片与账户菜单） | 身份、地址、可发送性 | Owner、委托或权限来源 |
| 本地草稿 | 保存/冲突/冻结/恢复状态 | 已接受或已投递 |
| 提交 | 已接受、对账中、失败或未知 | “已送达” |
| Agent 复核 | 真实策略或审批原因、可执行下一步 | 自动决策结果或审批人 |
| 无权限 | 受限原因与返回路径 | 可点击但最终失败的操作 |

## 5. 组件边界与实现方法

设计系统拥有跨应用一致的基础能力，邮件端和管理端只组合它们，不各自发明视觉基础。

| 层级 | 责任 | 当前代码入口 |
| --- | --- | --- |
| Token layer | 色彩、字体、间距、焦点、语义状态 | `web/design-system/src/styles.css` |
| Primitives | Button、IconButton、Control、Panel、Badge、EmptyState、IdentityBadge、StatusIndicator | `web/design-system/src/index.tsx` |
| Icon registry | 原创功能图标和实体图标 | `web/design-system/src/icons.tsx` |
| Product composition | 邮件目录、队列、详情、写信、真实状态编排 | `web/mail-app/src/` |
| Admin adoption | 审批、治理、审计的同 token 接入 | `web/admin-app/src/` |

实现新界面的顺序固定为：先定义/复用 token，再复用 primitive 或补齐可访问 primitive，
然后组合产品组件，最后接入真实 API 状态与测试。不得绕过设计系统在某个功能 CSS 中
临时硬编码色彩、阴影、图标或状态标签。

功能图标是手写 SVG，不是 emoji 或翻译字符串：24 × 24 viewBox、1.75px 描边、圆端点
和圆连接。每个可交互图标有本地化可访问名称；纯装饰 SVG 才标记为 decorative。生成
位图不得降级为 16px 操作图标。

共享 `Dialog`/`ConfirmDialog` 还统一承担 Portal、`aria-modal`、背景 `inert`、Tab
环绕、Escape、busy 锁定和焦点恢复。遮罩关闭必须通过完整指针手势判定：只有 pointer
down、pointer up 与 click 都发生在同一遮罩上时才关闭；从对话框内容拖到遮罩不得误关，
也不得让一次拖拽绕过未保存内容或危险操作的确认边界。

## 6. Image 2 位图资产生命周期

Image 2 只用于有明确语义的登录、引导、空状态或人工复核情境。当前获准使用的素材、
文件名、哈希、Alt 文本和页面映射以
[Image 2 素材清单](IMAGE2_ASSET_MANIFEST.zh-CN.md) 为准：

- `mail-agent-routing-v1.webp`：登录/首次接入主视觉；
- `inbox-zero-v1.webp`：收件箱真实为空时的情境插画；
- `agent-human-review-v1.webp`：Human 写信窗口收到真实 `manual_review` 提交状态并等待管理员复核时的情境插画；不得用于普通发送失败、未知提交或通用 Agent 状态。

PNG 是可审计源文件，WebP/AVIF 是产品交付格式。新资产必须经过以下路径才可合入：

1. 为一个具体的产品状态和一个可执行下一步写生成约束；
2. 目检是否含文字、Logo、水印、第三方轮廓、伪 UI 或无语义装饰；
3. 生成压缩交付文件，记录源文件 SHA-256、Alt 文本、使用页面和尺寸预算；
4. 在真实数据、中文、英文、空状态和窄屏下截图回归；
5. 在素材清单登记后再被代码引用。

不要提前生成或放置“搜索失败”“无权限”“域名未接入”等未来状态的通用图。对应 API
和操作尚不存在时，使用清晰文字和现有动作；等状态与动作真实存在后，再单独生成。

## 7. 可访问性与键盘契约

- 满足 WCAG 2.2 AA 对比度；焦点始终可见；不以颜色作为唯一信息通道；遵守减少动画
  偏好。
- 共享门禁会计算信息小字、控件轮廓和 Agent 焦点信号的关键 token 对比度。默认焦点为
  表面色隔离层加 Agent 色外环的不透明双环；`prefers-reduced-motion: reduce` 下 Spinner
  必须完全静止，不能以极短时长继续无限旋转。
- 焦点顺序为 Rail → Queue controls → Queue rows → Detail actions/content → transient
  UI。对话框必须圈闭焦点，关闭后回到触发控件。
- 鼠标和键盘必须驱动同一状态机。首批已有操作的快捷键是：`c` 写信、`/` 搜索、
  `j`/`k` 选择、`Enter` 打开、`Escape` 返回/关闭、`r` 回复、`s` 星标、
  `g i`/`g d`/`g s` 前往收件箱/草稿/已发送。输入框、组合键和无关浏览器快捷键不得
  被劫持。
- 所有异步状态必须可由屏幕阅读器理解：加载、刷新结果、错误、只读、提交冻结和
  焦点恢复均需有适当语义，而不只改变视觉。
- 邮件端在不超过 1007px 时，工具栏、返回、导航、筛选与其他可操作控件的命中区至少为
  44 × 44px；这一规则不依赖 `pointer: coarse` 等设备特征，桌面端继续保持紧凑密度。

## 8. 开发与验收门禁

以下是最终设计验收矩阵。在矩阵逐步自动化期间，每次设计变更合并前必须保证已经落地的
门禁不回退，并在实施状态中明确尚未覆盖的场景；不得用局部自动化结果冒充整张矩阵完成：

1. Token 定义层以外不新增原始色值；没有无语义插画、占位文本、假数据或死控件；
2. 对 Human/Agent/Shared/Group/System 的身份均有文字、图形和状态边界，不靠颜色区分；
3. 每个加载、空、错误、受限和未知状态均关联真实 API 条件与可执行下一步；
4. 关键流程可仅用键盘完成，焦点和屏幕阅读器语义完整；不超过 1007px 时邮件端关键
   触点不少于 44px，且不能只依赖指针类型媒体查询；
5. 在 1440、1280、1024、768 和 390px 回归截图，覆盖中英文、长主题、多收件人、
   附件、只读邮箱、空数据、错误和未知提交；
6. Image 2 资产通过清单登记和生产 Bundle 单文件体积预算：2× 情境/空状态 ≤ 120 KiB，
   2× 登录主视觉 ≤ 300 KiB；未知 WebP/AVIF 按更严格的 120 KiB 拒绝；
7. 现有 TypeScript 检查、单元测试、构建，以及安全/异步状态测试保持通过；生产构建还须
   通过 `npm run bundle:check`，不得包含 `.map` 或 JavaScript/CSS `sourceMappingURL`；
8. 运行 `python scripts/check_design_governance.py --self-test`，保证 token、
   Image 2 素材清单、翻译伪图标和 `EmptyState` Alt 的可机械约束不回退；
9. 运行 `python scripts/check_documentation.py`，保证所有文档入口和本地链接有效。

`npm run bundle:check:self-test` 使用独立临时 fixture 验证上述 Bundle 规则会拒绝违规产物；
`bundle:check:admin` / `bundle:check:mail` 供两个 Docker builder 在各自产物生成后独立执行。
根目录 `npm run build` 仍构建两个应用并做一次总校验，不能用单应用命令替代完整合并门禁。
同源 Nginx 配置还必须由 `python scripts/check_compose_security.py --self-test` 验证：成功的
`/assets/` 才可获得一年 immutable 缓存，缺失资产必须精确返回 `404` 且 `no-store`，不能
回落到 SPA `index.html`；页面入口和其他响应保持 `no-store`。这是配置与静态门禁，不代表
镜像已经在 Docker 或 NixOS 生产环境完成部署验收。

浏览器基线由 `web/playwright.config.ts` 和 `web/e2e/` 实现。执行 `cd web && npm run e2e`
会构建两个应用、检查 E2E TypeScript、启动仅监听 `127.0.0.1` 的 Vite Preview，并用完全
本地的 BFF 夹具运行 82 个 Chromium 测试（Admin 53、Mail 29）。Web Vitest 共 390 条
（共享设计系统 16、Admin 195、Mail 179）。当前 66 张 Windows 基线由 Admin 34 张、Mail
32 张组成：两端覆盖五档响应式布局、中英文、列表—详情、错误/空/只读等已实现状态；Mail
还覆盖搜索无结果、详情 `503`、草稿 CAS 冲突和真实 `manual_review` 证据锁定，Admin 覆盖
Cases、Holds、Approvals、Audit 等治理切片。非截图场景补齐 `/`、`c`、`j`/`k`、`Enter`、
`Escape`、`r`、`s`、`g i`/`g d`/`g s` 首批键盘路径、四类 mailbox 绑定、草稿精确 CAS，
以及 Admin Session 失效重校验、按身份隔离的终态证据、未知写入冻结/对账和按需路由失败恢复。
测试还在 390px 默认 fine pointer 下实际测量已实现控件的 44×44px 命中区，并对多个代表性
Mail/Admin 状态执行严格的 Axe A/AA 自动扫描。浏览器上下文只允许当前回环 Origin，未声明
API 或任何外部网络请求必须失败，测试不得访问真实 IdP、数据库、CDN 或邮件系统。Linux CI
继续独立执行类型检查、Vitest 和生产构建；像素比较由 Windows CI 承担，不能把当前 PNG
宣称为跨操作系统渲染基线。
快照只能通过 `npm run e2e:update` 显式更新并人工审图，CI 不自动接受变化。

这套自动化只证明同源固定夹具和已列页面/状态，不证明真实 RBAC、租户隔离、后端 CAS/
幂等或真实依赖，不能据此声称完整 WCAG 2.2 AA、真实触屏、屏幕阅读器或企业 IdP/邮件
环境已经验收。后续浏览器切片应继续覆盖其余 Admin 信息架构；真实触屏、屏幕阅读器及真实
IdP、控制平面、数据库和邮件系统依赖仍须单独验收。

设计评审不是只看一张桌面截图。评审者应同时核验真实状态、权限收敛、键盘路径、
窄屏行为、翻译长度、可访问名称和审计可追溯性。

## 9. 相关文档

- [设计契约（English source of truth）](../DESIGN.md)
- [V2 视觉重构与开发规格](DESIGN_DEVELOPMENT_SPEC_V2.zh-CN.md)
- [D0 Admin Shell 与 Overview 字段映射](DESIGN_D0_ADMIN_FIELD_MAPPING.zh-CN.md)
- [设计开发蓝图（P0 实施切片）](DESIGN_DEVELOPMENT_BLUEPRINT.zh-CN.md)
- [设计治理门禁](DESIGN_GOVERNANCE.zh-CN.md)
- [人类与 Agent 交互规范](HUMAN_AGENT_UX_SPEC.zh-CN.md)
- [Image 2 素材清单](IMAGE2_ASSET_MANIFEST.zh-CN.md)
- [Human 邮件应用安全边界](HUMAN_MAIL_APP.zh-CN.md)
- [管理端说明](ADMIN_APP.zh-CN.md)
- [企业实施计划](IMPLEMENTATION_PLAN.zh-CN.md)
