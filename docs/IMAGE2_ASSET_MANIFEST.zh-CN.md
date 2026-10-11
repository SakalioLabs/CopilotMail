# CopilotMail Image 2 素材清单

本清单记录可以进入产品的生成式位图素材。功能图标、徽标、按钮、图表和文字图形
不由 Image 2 生成；它们由代码和 SVG 系统控制。

## 已选定素材

| 文件 | 用途 | Alt 文本 | 源 PNG SHA-256 | 交付文件 SHA-256 |
| --- | --- | --- | --- | --- |
| `web/design-system/assets/illustrations/mail-agent-routing-v1.png`<br>`web/design-system/assets/illustrations/mail-agent-routing-v1.webp` | 登录与首次接入主视觉 | 装饰性图像；登录页容器为 `aria-hidden`，图片使用空 Alt | `4B6663BD6255E3754E8F79BC0DB54C919C20EEFE3D08C152FFA2AD667EB174AF` | `51A4A28C4B3E7883F67A964D100596EB832CED99D60E8CAF72C80842D81B0946` |
| `web/design-system/assets/illustrations/inbox-zero-v1.png`<br>`web/design-system/assets/illustrations/inbox-zero-v1.webp` | 收件箱为空时的情境插画 | `已清空的邮件托盘` | `BA00BBE009BB205C39909A498F5694FDFE7C4B5195EFAB003BE7BBDE92C0A75E` | `3A17431D99B7B359CE7146A88060854242C71A38D09768F4780587ECFD1A8B3F` |
| `web/design-system/assets/illustrations/agent-human-review-v1.png`<br>`web/design-system/assets/illustrations/agent-human-review-v1.webp` | Human 写信窗口收到真实 `manual_review` 提交状态、等待管理员复核时的情境插画 | `在复核关卡暂停的邮件处理路线` | `DC8B4D9A9E957F3AFFDF6FFE0C35E230F4FA010905A2A157BE1CFD68AB5C7EF5` | `E1569AB8AD35EB32D27D959A3DD66623F29FA1A7664DD75C920CB1FF889CF268` |
| `web/design-system/assets/illustrations/dual-principal-gateway-v2.png`<br>`web/design-system/assets/illustrations/dual-principal-gateway-v2.webp` | 会话已确认且服务器已返回可访问邮箱范围后的首次接入说明；不用于未登录或错误页 | 装饰性图像；说明文字本身表达 Human 与 Agent 共同连接到受验证邮件网关，图像容器为 `aria-hidden` | `8E61E6F536CE460D8BE7748714A393705B662E1584403685919D7714610E1C0F` | `D4247017A8F4CB2A54B23E07D01AFF7FF714FDDEF673454F495017E21573BEBD` |
| `web/design-system/assets/illustrations/audit-evidence-chain-v2.png`<br>`web/design-system/assets/illustrations/audit-evidence-chain-v2.webp` | 已获 `audit.export` 权限、但尚无当前 export grant 时的审计导出首次说明；仅和真实 Archive/Approval 输入同屏使用 | `由邮件归档、证据摘要、复核与验证标记组成的证据链示意图` | `C003A36ACEAFE0E412CA504A1B3187F10107C9E73256C5F1B45BD0B80A1B539C` | `5D60059701C9D6938AF731080FC6CCE9873C6740833A365948F2307909EFA539` |

## 共同生成约束

- 使用原创黑墨线稿、受控点阵纹理、暖象牙白底和极少量深靛蓝信号。
- 禁止文字、字母、数字、QR 码、Logo、水印、UI 截图、人脸、机器人表情、品牌化轮廓。
- 禁止渐变、光晕、炫光、黑底、金属/3D 质感、彩色拼块和无语义背景物。
- 一张图片只绑定一个产品状态和一个下一步动作。没有绑定的素材不进入代码；已登记但
  尚未被源码引用的素材，只能作为已定义真实页面/状态的待接入资产，不能借此出现在
  不相关的空态或错误页。
- 生成原文件保留在本地生成目录；仓库只保存经过目检、命名和哈希登记的定稿副本。

## 尚未生成、不得提前占位的素材

- 搜索无结果。
- 域名未接入。
- 不属于 `manual_review` 的邮件发送失败。
- 无权限查看 Agent 证据。

这些状态在对应 API 与页面存在后，以同一约束单独生成；不以复制当前插画、假数据或
占位框代替。

## 发布前优化要求

- 生成 PNG 是审计源文件；WebP/AVIF 是产品交付文件。
- 情境/空状态 2× 成品不大于 120 KiB，登录主视觉 2× 成品不大于 300 KiB；当前 WebP
  交付分别为约 44KB、21KB 与 30KB。
- 优化后同时登记源 PNG 与产品交付文件的 SHA-256，并保持尺寸、Alt、使用页面与视觉回归截图记录；交付文件被重新编码后必须更新清单并重新目检。
- `npm run bundle:check` 会对生产 `dist` 中每个 WebP/AVIF 单独执行上述体积上限；当前
  `mail-agent-routing-v1` 是唯一登记为 300 KiB 上限的登录主视觉，其余以及无法识别
  分类的新位图一律按 120 KiB Fail Closed。`npm run bundle:check:self-test` 用临时违规
  WebP/AVIF 证明该规则生效，不依赖也不改写已有生产产物。
