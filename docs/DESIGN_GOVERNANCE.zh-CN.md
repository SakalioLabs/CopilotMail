# CopilotMail 设计治理门禁

`python scripts/check_design_governance.py --self-test` 是设计系统的轻量、
Fail-Closed 静态门禁。它不替代浏览器截图、可访问性或产品评审；它只阻止那些可以从
源码可靠证明的设计回退，并在 Windows、Linux 开发机和 CI 上仅依赖 Python 标准库运行。
真实浏览器证据由 `web/playwright.config.ts` 与 `web/e2e/` 单独承担。

## 运行方式

```bash
python scripts/check_design_governance.py --self-test
python scripts/check_documentation.py
python scripts/check_compose_security.py --self-test

cd web
npm ci
npm run bundle:check:self-test
npm run e2e:install
npm run e2e
```

CI 在 Web 类型检查、测试和构建之前执行静态门禁。设计治理 `--self-test` 覆盖 CSS token
边界、V2 共享字体/间距/字号/动效/触点/层级 token 的存在性、关键 token 对比度与减少动画
契约、素材清单解析、`EmptyState` Alt 属性、原生浏览器 Dialog 和翻译文案中的伪图标检测，以及生成的
`test-results`、`playwright-report`、`blob-report` 不得被误扫的目录边界，随后才检查仓库；构建
完成后，独立 Windows CI 安装固定 Playwright 版本对应的 Chromium，并在与 `-win32` PNG
基线相同的操作系统族中运行浏览器回归；Linux 开发机使用独立的 `-linux` 基线（
`snapshotPathTemplate` 含 `{platform}`，2026-10-12 起）。Linux CI 继续独立执行类型检查、
Vitest 与生产构建。失败时上传 `playwright-report` 与 `test-results` 诊断文件。

## 浏览器级回归基线

`npm run e2e` 先构建 Admin/Mail 两个 Vite 生产产物，再运行严格 TypeScript 检查和
Playwright。两个预览服务只绑定 `127.0.0.1:4173/4174`；测试在导航前安装本地
`page.route` BFF 夹具。浏览器上下文禁用 Service Worker 且只允许当前回环 Origin；任何
未声明的 `/api/**` 或外部网络请求都会使测试失败，因此不会访问真实 IdP、数据库、
邮件引擎、CDN 或公司 API。

当前浏览器上下文固定浅色、`en-US`、UTC、`deviceScaleFactor=1` 与减少动画偏好。2026-10-12
起两个应用的产品默认语言均为 `zh-CN`（无已存选择时一律渲染简体中文，不再按浏览器语言回退
`en`）；为让选择器文本与视觉基线不依赖宿主机，Admin/Mail 夹具都在首次渲染前向产品自己的
Locale 存储键写入显式 `en` 选择，已有选择（例如测试中经可见语言切换产生的选择）不被覆盖。
中文场景继续通过产品内真实语言控件进入 `zh-CN`，并验证 `<html lang>`、严格 Locale 持久化和
刷新后恢复。2026-10-12 的 Gmail 风格外壳改版（见下文「2026-10-12 设计语言」）之后的当前本地
回归中，`npm run check`、受限 worker 的 Vitest、`npm run build` 与 `npm run e2e:run` 均通过：
Web Vitest 共 428 条（共享设计系统 19、Admin 217、Mail 192），Chromium 共 122 条，并按平台
保留 205 张 PNG 基线（`-linux` 102 张：Admin 39、Mail 63；`-win32` 103 张：Admin 40、Mail 63）。
这些同源固定夹具的回归覆盖：

- Mail 已授权 Human Mailbox 的真实 DTO 结构、长主题、多收件人、附件和详情，在
  1440、1280、1024、768、390px 五档进行截图比较；
- Mail 简体中文工作台在相同五档进行截图比较，并验证 `<html lang>` 与持久化 Locale；
- Admin 单 Agent Inspector 的显式 UUID 查询结果以英文、简体中文分别在相同五档进行
  截图比较；畸形和保留 UUID 在本地拒绝且不发请求，`403`、统一 `404`、按
  `retryable` 决定是否重试的 `503` 及刷新失败保留旧快照/焦点均有浏览器契约，响应中的
  内部错误、Trace 和敏感细节不得进入页面；
- Admin Cases、Holds、Approvals、Audit 在 1080/720px 各保留一张详情图，并验证列表与
  详情真实互斥、Back/Escape 精确焦点恢复、1440 双栏、1081 无页面级溢出及 Approval
  迟到详情不能重开已返回的选择；
- Admin Session 在过期、窗口重新聚焦与受保护请求 `401` 后重新校验；身份/失效时间变化
  会重建工作区并清空上一操作者证据。Cases/Holds 的终态账本与已提交证据可在同一 Session
  内跨页面保留，严格响应解析会绑定租户、目标 ID、筛选和分页；未知写入结果先冻结精确请求。
  审批决策和取消仅按已有请求 ID 执行精确 GET 对账；审批创建、案件创建、Hold 施加等未返回
  资源 ID 的未知写入保持冻结，不用列表猜测候选对象且禁止再次 POST。仅已有精确幂等契约的操作
  才可重放完全相同的请求。重型路由加载失败进入可重建的安全恢复边界，不显示原始异常；
- Retention 创建使用按需 `Dialog`，并以明确控件表达可空字段语义，不依赖示例 UUID 或空白
  placeholder 传达持久业务含义；共享 Dialog 的浏览器/单元契约还证明从内容区拖向遮罩不会
  被误判为遮罩点击而关闭；
- Mail 的 Shared 只读权限收敛、真实空收件箱、`503` 错误与可见重试、未知提交冻结及
  精确请求重放，以及搜索专用无结果、详情 `503` 原位重试、草稿恢复 `409` 冲突，均有
  对应状态契约；真实 `manual_review` 还锁定精确 Submission UUID 与写信内容，关闭只隐藏
  证据、重新打开仍恢复同一冻结上下文，并只在该真实状态显示登记的 Image 2 人工复核素材；
- Mail 邮箱切换收纳在身份卡片账户菜单中，按 Human、Agent、Shared、Group 四类真实 `role="group"` + `menuitemradio` 分组并标记当前项；切换到 Agent
  后，身份、地址与后续收件箱请求精确绑定所选邮箱；已有本地编辑内容时恢复草稿必须先
  确认覆盖，首次 `409` 后刷新到 metadata v5，再只用该精确版本执行 CAS 恢复；
- Mail 的 `/`、`c`、`j`/`k`、`Enter`、`Escape`、`r`、`s`、`g i`/`g d`/`g s`
  完整首批键盘路径，以及 Admin 查询/刷新与焦点保持；
- D4A Mail 在 1084/1083/1008/1007px 的 rail、Queue、Reading 与列表—详情切换几何；移动
  抽屉通过身份卡片账户菜单提供邮箱切换、语言与退出入口，未展开菜单时不发起
  详情预取；
- D4B Queue 在 1440/1008/390px 下的真实扫描层级、未读非色彩线索和无横向溢出；`j`/`k`
  不产生详情读取或状态写入，Reading 的真实元数据与既有动作行位于安全正文之前；
- D4C Drafts/Sent 在 1440/1008/390px 下的 metadata-only 宽列表、无横向溢出和无假详情；
  目录切换不读取消息详情/写入消息状态，Draft/Sent 各执行 Axe A/AA，Compose 的 unknown 与
  manual-review footer 长状态完整换行而不裁剪；
- D4D Compose 在 1440/1008/390px 下只保留两项授权 Context facts，并把静态 `can_send` 放入
  ContextBar status 槽（无 `aria-live`）；正常写信、unknown、manual review 展开证据和恢复草稿
  各有实际基线，D4D Compose 执行 Axe A/AA，未知提交仍只确认同一幂等上下文；
- D4E 的英文/简体中文登录，以及英文的已授权目录为空、目录暂不可用和会话复核在
  1440/901/390px 各有固定夹具：登录 Hero 只出现于登录入口，目录为空/暂不可用不伪装为登录，
  复核失败为聚焦的非模态 alert，底层工作台保持 `inert` / `aria-busy`；每个场景同时验证
  Axe A/AA、无横向溢出、44px 触点及无额外 Mail 资源读取；
- 390px、默认 fine pointer 下对已实现的 Scope 披露、邮箱、语言、工具栏、导航、搜索、详情、
  命令面板和写信控件进行实际几何测量，要求命中区不小于 44×44px；
- 多个代表性 Mail/Admin 页面与关键状态执行严格的 Axe A/AA 自动扫描；这些扫描是固定夹具
  下的回归证据，不构成完整 WCAG 2.2 AA 验收。

基线只能通过 `npm run e2e:update` 显式更新，且每个平台各有一份：`{arg}-win32.png` 供
Windows CI、本机人工审图的 `*-linux.png` 供 Linux 开发机；更新任一侧都必须人工查看对应
平台的全套 PNG（win32 103 张：Admin 40、Mail 63；linux 102 张：Admin 39、Mail 63）：
Admin 40 张，覆盖英文/简体中文五档布局、分组导航、域名治理、P2 调查/受控观察与治理列表—详情状态；Mail 63 张，除既有响应式、
搜索无结果、详情错误与草稿 CAS 冲突外，还覆盖真实 `manual_review` 全页状态及严格 footer
局部基线、D4A 390px 移动 Scope 披露基线、D4B Queue 1440/1008/390px 和 768px Scope
披露基线，以及 D4C Drafts/Sent 1440/1008/390px 基线；另含 D4D 正常
Compose 1440/1008/390px、unknown 1008px、manual review 展开证据与恢复草稿三档基线；D4E 的
英文/简体中文登录、目录为空、目录暂不可用和会话复核失败各在 1440/901/390px 的 15 张基线。人工确认
没有布局溢出、错误信息层级、占位数据或身份/
状态误导后再提交快照；CI 只比较，不自动接受新图。固定夹具仅存在于 `web/e2e/`，不会
进入生产 Bundle。

## 被机械保护的契约

| 契约 | 检查方式 | 失败后的修复方向 |
| --- | --- | --- |
| 功能 CSS 不引入原始颜色 | 扫描 `web/*/src/**/*.css`；十六进制、数值色彩函数和 CSS 命名颜色只允许出现在 `web/design-system/src/styles.css` 的 `@layer tokens` 块中。注释与字符串不参与判断。 | 在共享 token layer 定义语义 token，再由功能 CSS 使用 `var(--cm-…)`。 |
| V2 基础 token 不分叉 | 共享 stylesheet 必须定义本地优先的中英文 `--cm-font-sans`、`--cm-font-mono`、4px/8px 间距、语义字号/行高/字重、44px 触点、dialog 半径、120/160/200ms motion、easing 与统一 z-index token；根与 code-like 内容必须实际消费两套字体 token。 | 在 `web/design-system/src/styles.css` 的唯一 token layer 增补或修复；不在某个应用 CSS 中临时造 private token。页面级 spacing/type 迁移仍按 D3/D4/D8 分批实施。 |
| 关键视觉可访问性 | 计算信息小字、控件边界和 Agent 焦点信号的 WCAG 对比度；焦点 token 必须是表面色加 Agent 色的不透明双环；减少动画模式必须限制迭代并彻底停止 Spinner。 | 调整共享语义 token 或 reduced-motion 规则，不以半透明视觉效果规避门槛。 |
| Image 2 位图可追溯 | `web/design-system/assets/illustrations/` 中的 PNG/WebP/AVIF 都必须在 [Image 2 素材清单](IMAGE2_ASSET_MANIFEST.zh-CN.md) 中以完整路径登记；审计源 PNG 与生产交付 WebP/AVIF 分别核对 SHA-256。生产源码引用该目录时，必须指向已登记的交付文件。 | 先目检并登记用途、Alt、源文件/交付文件哈希和两个完整路径，再接入页面。 |
| 生产 Bundle 与 Image 2 交付预算 | `npm run bundle:check` 分别读取两个应用的真实 `dist`，限制首屏/总 JavaScript、CSS 及 gzip 体积，拒绝任何 `.map`、JS/CSS `sourceMappingURL`，并逐个限制 WebP/AVIF：情境图不超过 120 KiB，登记的登录主视觉不超过 300 KiB。P2 收敛时 Admin 的测量为首屏 JavaScript 317,699 B（gzip 97,134 B）、完整图谱 JavaScript 459,423 B、CSS 48,927 B。P3 增加按需打开的 Agent Inspector 与 Human-only、可审计的精确邮件提交证据工作区后，曾测得首屏 JavaScript 329,319 B（gzip 100,261 B）、完整图谱 JavaScript 479,722 B、CSS 48,927 B（gzip 9,006 B）。截至 2026-08-24，D3 的全/窄/抽屉导航和事实→决策 Overview 没有引入 CDN、运行时样式或位图依赖；当前 Admin 生产工件为首屏 JavaScript 335,950 B（gzip 102,427 B）、完整图谱 JavaScript 490,690 B、CSS 52,877 B（gzip 10,035 B）。因此完整图谱 JavaScript 仅对 Admin 明确设为 480 KiB（491,520 B），原始 CSS 仅对 Admin 明确设为 53 KiB（54,272 B）；首屏 JavaScript、gzip CSS（12 KiB）和 Mail 的所有预算仍保持共享默认值，Mail 完整图谱 JavaScript 仍为 420 KiB（430,080 B）。每个例外均有预算脚本自测，绝不是后续功能预留的隐式增量：再次上调必须在本表和预算脚本中单独说明理由。`bundle:check:admin` 与 `bundle:check:mail` 可独立校验单一产物；两个生产 Docker builder 在复制产物前强制执行相应命令。 | 不得通过删除门禁、把翻译/业务数据移到运行时网络请求、或改成只看开发源码来放行；先移除无效依赖、拆分非首屏模块、关闭 source map 或重新压缩交付位图。Admin 的 480 KiB 与 53 KiB 只适用于其完整、可选的受控工作区图谱，不放宽首屏约束，也不适用于 Mail；P3/D3 的例外不构成后续自动扩容依据。新增主视觉前必须显式登记分类，未知位图默认采用更严格的 120 KiB 上限。 |
| 翻译不承担图标职责 | 仅扫描运行时 locale catalog（`web/*/src/i18n.ts` 与 `web/*/src/locales/`），拒绝 emoji、dingbat 与常用符号图标，包括 TypeScript 的 Unicode escape。破折号、中文标点、正文和邮件内容不在扫描范围。 | 使用 `@copilotmail/design-system` 的 SVG icon registry；翻译只提供可读名称与文案。 |
| 有插画的 `EmptyState` 必有 Alt | 检查从 `@copilotmail/design-system` 的命名导入（含别名）和 namespace 导入的 JSX `EmptyState` 调用。发现 `illustration` 时必须显式提供 `illustrationAlt`。 | 传入对应 locale 的 Alt；刻意装饰时也要明确传入空 Alt，而不是遗漏属性。 |

### 2026-10-12 设计语言

按 2026-10-12 用户要求（评审此前 Linux 截图后认为旧外观"有点丑"），Mail 与 Admin 已统一为
Gmail 风格外壳，仅表现层与布局改动，API 调用、权限、审计与业务逻辑不变：

- **共享外壳**：顶部栏承载纯文字字标（`app-wordmark`，无图片/SVG 图形 Logo）、大圆角
  `search-omnibox` 搜索框与语言/账户区；左侧导航栏使用共享 `app-nav-item` 胶囊条目（选中项
  浅色高亮、未读计数徽标），Mail 顶部另置圆角「撰写」主按钮。Mail ≤1007px、Admin ≤1279px 时
  导航收进原生 Dialog 抽屉，Mail 的撰写入口改为右下角悬浮按钮；同一抽屉内保留语言与退出控件，
  不丢失任何功能入口。
- **视觉令牌**：白底 `--cm-surface`、浅灰分隔 `--cm-line`、胶囊/8–16px 圆角、轻阴影
  `--cm-shadow-1/2`、4/8px 间距栅格与语义字号集中在 `design-system/src/styles.css` 的
  `@layer tokens`；WCAG AA 对比度、双环 `--cm-focus-ring`、44px 触点与键盘导航契约不变。
- **Admin 专用组件拆分**：Mail 从不挂载的 `Panel`/`Badge`/`Field`/`EmptyState`/`Spinner`/
  `UnknownWriteLock` 样式移入 `design-system/src/admin.css`（仅 Admin 经
  `@copilotmail/design-system/admin.css` 引入），共享层名不变、级联一致；减少动画的 Spinner
  静止保证仍留在共享 `styles.css` 的同一 media 块中。拆分为预算卫生措施，不构成第二套
  视觉语言，Admin 侧规则仍只消费共享 token。
- **i18n**：两个应用默认 `zh-CN`，`en` 为第二语言；语言切换在顶栏（宽屏）与抽屉（窄屏）可见、
  可键盘操作并写入各自 `copilotmail.*.locale.v1` 存储键。`scripts/check_web_i18n.py` 及其
  AST 检查覆盖 mail-app、admin-app 与 design-system，失败即关闭；两端单测断言 zh-CN 与 en
  键集合完全一致且无空值。
- **视觉基线**：改版产生的像素差异为预期；本机已按 `--update-snapshots --workers=1` 重建
  `-linux` 基线（102 张）并人工审图。`-win32` 基线保持原样未动：Windows 环境的视觉比对在
  重新生成前会失败，需待 Windows CI/机器按同一流程重建。

### 2026-10-13 控件精修（批次 16）

用户 2026-10-13 08:39 复核批次 14 截图后反馈「设计方向没问题，就是控件质量不行，优化一下」。
本批只精修控件质感、对齐 Material 3 / Gmail 的交互语法；信息架构、API、权限、审计与业务逻辑
不变，功能入口一个不少：

- **共享控件语法**：按钮（主/次/文字/图标按钮）、`.control` 输入框与下拉（统一边框、
  chevron、placeholder 与 disabled 表现）、`search-omnibox`、`app-nav-item`、
  `topbar-sign-out` 与 `EmptyState` 收敛到 `design-system` 的同一交互语法。Mail 的
  LanguagePicker/MailboxPicker 下拉与 retry、load-more、compose、reply、composer-send、
  sign-out 等按钮改复用共享 `.control`/`.button` 变体；MessageList/DraftList/SentList 的
  空态改用共享 `EmptyState` 组件（标题层级 2，保留 e2e 依赖的 `.empty--actionable`/
  `.empty-action` 钩子）。Admin 的 locale-switch、表格选中、工具栏、筛选 Chip 与 Toast
  同样改读共享 token 与状态层；两端 bespoke 圆角并入共享半径档。
- **令牌**：新增 Material 风格状态层与控件几何 token——`--cm-control-height`、
  `--cm-icon-size`、`--cm-select-indicator`、hover/pressed 状态层色与 disabled 色，全部
  集中在 `@layer tokens`；`.button` 变体改为重定向共享 `--_hover`/`--_pressed` 状态层，
  不再各自手写 hover 色。
- **命令面板拆分**：Mail-only 命令面板样式移入新的 `design-system/src/mail.css`（与
  `admin.css` 同模式、仅 Mail 引入），保证 Admin CSS 预算仍在既有上限内；共享层名不变。
- **修复**：`.compose`/`.compose-fab` 组件层静态阴影会遮住 reset 层焦点环，补显式
  `:focus-visible` 覆盖恢复双环（mail 焦点边界 e2e 契约断言）；Admin 侧栏底部「语言」
  标签加单行约束，避免在窄栏折行。
- **视觉基线**：`-linux` 基线重建 78 张并人工审图（无错位/截断/重叠）；`-win32` 基线
  保持原样，仍待 Windows 环境按同一流程重建。
- **保持不变**：移动端触点选择器与 e2e DOM 钩子全部保留；WCAG AA 对比度、双环
  `--cm-focus-ring`、44px 触点、键盘导航与 Axe 契约不变，没有放宽预算或断言。

### 2026-10-13 视觉元素丰富（批次 17）

用户 2026-10-13 复核批次 16 截图后反馈「有点素」，并选定方向「加插图和视觉元素：空状态插画、
图标、卡片阴影」。本批同样只改表现层：信息架构、API、权限、审计与业务逻辑不变，功能入口
一个不少，不新增依赖、不引入图标库/远程字体/远程图片：

- **空状态插画**：`design-system/src/illustrations.tsx` 提供 11 个产品自制场景
  （inbox/search/drafts/sent 无结果、未选中消息、未选中记录、审批、审计、合规案件与
  Legal Hold 共用、域、角色），统一 200×150 viewBox、消费 `--cm-*` 语义色而非原始色。
  `EmptyState` 新增 `art` prop：传入 ReactElement，组件克隆时附加
  `empty-state__illustration` 类；所有场景自带 `data-empty-art`、
  `aria-hidden="true"`、`focusable="false"` 装饰属性。Mail 收件箱/搜索/草稿/已发送/
  未选中消息与 Admin 审批/审计/合规/域/角色/详情侧栏均已接入；位图 `illustration`
  通道（含 Image 2 登记约束）保留不变。
- **图标组件化**：`design-system/src/icons.tsx` 的 41 个自制 glyph 拆分为独立导出组件
  （`IconSvg` 共享外壳统一尺寸与 `aria-hidden`），名称驱动的 `Icon` 与 `ICON_REGISTRY`
  保留给 Mail 的动态查找。图标全部为本仓库原创路径数据（product-owned），未使用任何
  第三方图标库路径或字体，因此无外部许可证义务；如未来引入宽松许可（MIT/Apache-2.0/ISC）
  的路径数据，必须在本节登记来源。`IconButton` 接受图标元素而非名称，DS 内部组件
  （StatusIndicator/IdentityBadge/EmptyState 回退/dialog/context-bar/unknown-write-lock/
  command-palette）均直接引用独立组件，Admin bundle 不再携带完整注册表。
- **品牌与卡片层次**：`BrandMark` 独立为 `design-system/src/brand-mark.tsx`（字符 Logo
  色块），用于两端顶栏与 Admin 登录卡。Mail 队列/阅读面板获得 `shadow-1` 静息阴影，
  Admin 总览指标卡加图标 chip（`overview-fact__mark`）与悬停 `shadow-2` + 顶部品牌色
  强调线；圆角/阴影仍全部来自既有 token 档，未新增色值。
- **Admin 去重**：`web/admin-app/src/format.ts` 集中日期/shortId/焦点环辅助函数
  （此前 8/5/3 处重复），AuditView 复用共享 `Pagination`，导航配置直接引用图标组件
  （删除 `AdminNavIcon` 间接层），ComplianceViews 内部 Fact 与 updateCreateDraft 收敛。
- **i18n**：新说明文案进入两端 locale 目录（zh-CN 默认、en 对齐）；装饰性 SVG 不出现在
  翻译里，仅图标按钮的 `aria-label` 仍走 i18n。
- **视觉基线**：预期像素变化；本机已按 `--update-snapshots --workers=1` 重建 `-linux`
  基线（7 张，均 mail）并人工审阅 22 张捕获截图无错位/截断/重叠。Admin 侧增量处于各
  断言的 `maxDiffPixelRatio` 阈值内、基线未变。`-win32` 基线保持原样，仍待 Windows 环境
  按同一流程重建。

### 2026-10-14 苹果风格控件与非线性弹簧动画（批次 18）

用户 2026-10-14 要求「UI 的控件风格看看能不能学一学苹果，然后增加一下非线性的弹簧动画」。
本批仍只改表现层与共享令牌：信息架构、功能入口、API、权限、审计、i18n 与批次 17 的
插画/图标/字符 Logo 全部不变，不新增依赖、不引入远程字体或图标库：

- **字体栈**：`--cm-font-sans` 改为 `-apple-system`/`BlinkMacSystemFont`/`"SF Pro Text"`
  优先，macOS/iOS 自动落到 SF；Inter、`"PingFang SC"`、`"Microsoft YaHei"`、
  `"Noto Sans CJK SC"`、`system-ui`、`"Segoe UI"` 等本地回退全部保留，不加载远程字体。
  Linux/CI 渲染与基线不变。
- **弹簧令牌**：`--cm-ease-spring-gentle`/`--cm-ease-spring-snappy` 为阻尼振荡器
  （gentle ζ≈0.72、约 4% 超调；snappy ζ≈0.55、约 12% 超调；采样公式
  `x(t)=1-e^{-ζωt}(cos ωd t + ζω/ωd·sin ωd t)`，ωd=ω√(1-ζ²)，gentle ω=10、
  snappy ω=16，各 16 采样点）。`@layer tokens` 中先给 cubic-bezier 近似回退，
  文件尾部 `linear()` `@supports` 探测块（不写进 tokens layer，天然具更高级联）把
  令牌升级为真实 `linear()` 弹簧曲线；`--cm-motion-spring-gentle`(380ms)/
  `-snappy`(240ms) 为 duration+easing 复合令牌。
- **弹簧应用**：`.button`/`.icon-button` 按下 `scale(0.97/0.9)` 回弹（transition 的
  transform 通道用 snappy 弹簧，其余通道仍走 `--cm-motion-interactive`）；
  `.evidence-inspector` 的展开箭头用 snappy 旋转；
  浮层 `.dialog`、Mail 撰写窗 `.composer`、Admin `.toast` 用
  `@keyframes cm-surface-in`（位移 10px + 轻微缩放 + 淡入，gentle 弹簧）；Admin 侧栏
  抽屉用 `cm-drawer-in`（translateX(-14px)，gentle）。
- **控件**：Admin `.checkbox-field` 的原生 checkbox 外观升级为 iOS 式 44×26 胶囊开关
  （`appearance: none` + `::before` 圆形 thumb，thumb 位移走 snappy 弹簧；role、
  label 关联、键盘与焦点环契约不变，label 行仍提供 44px 触点）；Audit 时间预设
  `.preset-buttons` 收敛为圆角分段控件组（容器 `surface-subtle` 底 + 2px 内边距，
  段为 quiet 按钮）。
- **减少动画**：全局 `prefers-reduced-motion: reduce` 契约不变——所有
  transition/animation 时长降到 0.01ms，弹簧与入场动画在该模式下等价于直接切换；
  e2e 浏览器上下文本就固定 reduce，动画不影响截图稳定性。动画只用
  transform/opacity，不引起布局抖动。
- **预算腾挪**：全部新增通过合并重复选择器与收窄 mail 私有别名
  （`--mail-touch-target-size` → `--mail-touch`，测试同步更新断言字面量、断言强度不变）
  自费；未提高任何预算或阈值。

### 2026-10-14 IMail 品牌、身份菜单、全宽收件箱与悬浮写信（批次 19）

用户 2026-10-14 需求：品牌改名 IMail、侧栏左下身份卡片与苹果式弹出菜单、收件箱
全宽+独立详情页、写信按钮改为右下角悬浮按钮。除命名与布局外，权限、审计、
业务逻辑与 i18n 目录结构不变，不新增依赖：

- **品牌**：用户面 wordmark 为粗体文字「IMail」（`brand-wordmark`/`app-wordmark`
  纯文本，无图片/外部字体），副标题与 ARIA 走 i18n（zh「智能邮箱」/ en "Smart Mail"），
  `<title>`、`document.title`、登录/目录页、会话校验文案同步。内部 crate/包名、
  API 开发者控制台与 Admin 平台面继续称 CopilotMail。
- **身份卡片与菜单**：导航区左下 `identity-card` 展示目录邮箱的显示名与地址；
  `identity-menu` 弹出层（`role="menu"`/`menuitem`、弹簧 `translateY+scale` 入场、
  仅 transform/opacity）提供「设置」「退出登录」两个菜单项；Enter/Space 打开、
  方向键循环、Escape 关闭并把焦点还给卡片、外部点击关闭。语言选择从顶栏/抽屉
  迁入「设置」对话框（`settings-dialog`，焦点返回卡片）。
- **全宽收件箱与详情路由**：`.mail-shell` 改为 rail + 单内容列；收件箱列表独占
  内容列。打开消息 `pushState` 到 `#/message/<encoded-id>`：详情视图可直达、可
  刷新、浏览器前进/后退与 deep link 均经 `hashchange`/`popstate` 同步；返回按钮
  恢复列表滚动位置并把焦点还给被选行。选中行在分页/刷新后按 message_id 重绑，
  消失时按原索引确定性地落到相邻行并关闭详情路由。
- **悬浮写信按钮**：侧栏与抽屉的 compose 入口收敛为单一 `.compose-fab`
  （56px、`position: fixed`、右下、`env(safe-area-inset-*)` 感知、可聚焦、
  本地化 aria-label），按下 `scale(0.92)` 经 snappy 弹簧回弹，仅 transform；
  列表/详情底部统一 `--mail-fab-gap` 间隙，不遮挡末行。
- **预算腾挪**：新增样式全部自费——合并 `.inbox`/`.detail` 卡片 chrome、
  `.identity-card`/`.command-trigger` 交互基底、跨组件省略号组与焦点环/悬停
  规则，抽 `--mail-fab-gap` 共享间隙变量；Mail CSS 49,131 B ≤ 49,152 B 默认线
  （余 21 B），未上调任何预算或阈值。

### D4A/D4B/D4C/D4D/D4E 当前生产构建测量（2026-10-14）

最新 `npm run build` 的 Admin 工件为：首屏 JavaScript 335,232 B（gzip 102,313 B）、完整图谱
JavaScript 491,296 B、CSS 54,215 B（gzip 10,478 B）。Mail 工件为：首屏/完整图谱 JavaScript
329,149 B（gzip 97,432 B）、CSS 48,880 B（gzip 9,210 B）；Mail 交付位图 2 个，最大
29,720 B。批次 18 的弹簧令牌、入场动画与 iOS 开关全部在既有预算内完成：Admin 完整图谱
JavaScript 距 480 KiB 例外线余 224 B（未变），CSS 距 53 KiB 例外线余 57 B；Mail CSS 距共享
48 KiB 默认线余 272 B（批次 17 余量为 3 B，腾挪后反而增大）。
D4E 只复用已登记的登录主视觉。表中较早 P2/P3/D3 与 2026-08-24、2026-10-12 测量仅保留为
历史演进记录；本节是当前构建事实。

素材清单的表格行是门禁的机器可读接口：每一行必须包含完整仓库路径、具体页面/
状态用途、Alt 文本与 64 位源 PNG SHA-256。不要把一个路径缩写成“同名 `.webp`”，
否则无法证明代码引用的交付文件已经登记。

Bun​​dle 门禁本身也有不依赖生产 `dist` 的负向测试：`npm run bundle:check:self-test`
在系统临时目录生成合法最小产物并执行 13 个负向 fixture 与 Admin/Mail 完整图谱预算
差异的专用断言，逐项证明 `.map`、JavaScript/CSS
内联映射声明、超限 WebP/AVIF、首屏原始/gzip JavaScript、总 JavaScript、CSS 原始/gzip、
错误 `modulepreload`、缺少模块入口和缺少静态导入均会 Fail Closed。该命令只删除自己刚创建的临时
目录，不读取或改写两个应用的生产构建结果。总校验及两个单应用校验都会先执行该自测，
避免门禁实现本身失效后仍接受生产产物。

同源 Nginx 的缓存与 SPA 边界由独立 Compose 安全门禁保护：只有成功的 `/assets/` 响应可
获得一年 immutable 缓存，资产缺失必须精确 `404` 且 `no-store`，不得回落到 `index.html`；
页面入口及其他响应保持 `no-store`。该脚本还以负向 fixture 验证注释伪指令、错误作用域、
竞争 `return` 和额外缓存头不能绕过检查。这里描述的是仓库配置门禁，不声称 Docker 镜像或
NixOS 生产部署已经通过动态验收。

## 有意保留的边界

静态脚本只证明已登记关键 token 的数学对比度，不能替代实际渲染审计。以下事项不能由
静态脚本可靠判定：所有组合与图像中的实际 WCAG 对比度、实际焦点序、屏幕阅读器体验、键盘
流程是否完成、真实 API 状态是否被如实表达、截图回归、第三方商标相似性，以及一张
插画是否真的具有产品语义。当前 Playwright 基线只证明上述同源固定夹具和已列状态；
它不证明真实 RBAC、租户隔离、后端 CAS/幂等或任何真实依赖，不等同于完整 WCAG 2.2
AA、真实触屏、屏幕阅读器或生产企业环境验收。下一步仍需扩大到更多 Admin 工作流，并
在真实触屏、屏幕阅读器及真实 IdP、控制平面、数据库与邮件系统依赖上验收。其余范围仍按
[设计系统开发规范](DESIGN_SYSTEM.zh-CN.md) 和 [Human / Agent 交互规范](HUMAN_AGENT_UX_SPEC.zh-CN.md)
评审。

本门禁刻意不扫描以下内容，避免将非 V2 功能样式或真实内容误判为设计回退：

- 根目录 `web/index.html` 中保留的 V1 原型内联样式；它不属于已接入设计系统的 V2
  Mail/Admin feature CSS。
- 依赖包、构建产物、测试 fixture、生成的 `test-results`/`playwright-report`/`blob-report`、
  邮件正文、Markdown 文档和普通业务字符串。
- 不在受控 `illustrations/` 目录中的非 Image 2 文件；它们仍应按其所属组件和代码评审。

若未来出现新的 CSS 预处理器、locale 目录结构、设计系统 re-export 方式或位图资产
目录，先扩展脚本与 `--self-test`，再迁移源码。不要用路径白名单或把某个违规字面量
加入例外来让门禁通过。
