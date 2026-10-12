# 决定记录 2026-10-12

> 状态：2026-10-12 用户授权监工按保守、安全、最小范围代为决定。本文件是
> 这些决定在仓库内的登记；执行证据以
> [实施状态](IMPLEMENTATION_STATUS.zh-CN.md)为准。

1. **B-26 多语言 SDK：不做（won't-do）。**用户明确不需要传统 SDK；
   [OpenAPI V2](openapi-v2.yaml) 与 [AsyncAPI V2](asyncapi-v2.yaml) 规范，
   加上现有无依赖 Python CLI 与可分发的 Agent Skill 已足够，避免维护多份
   生成代码。任何文档不得把该能力写成“多语言 SDK 已完成”。
2. **D5–D8 视觉收尾：不在本地硬做复合组件。**监工生成 Linux 截图供用户
   审图，按用户反馈再开批次。现行判定见实施状态与 BACKLOG B-27。
3. **Linux 截图基线：接受。**`web/playwright.config.ts` 的
   `snapshotPathTemplate` 加 `{platform}` 后缀；现有 Windows PNG 以
   `git mv` 重命名为 `-win32` 版本保留，新增 `-linux` 基线；不改
   `maxDiffPixelRatio`，不删用例。Windows CI job 继续使用 `-win32` 基线。
4. **`source-map-js`（GHSA-68fv-2mgg-jv7q）：升级到已修补版本。**只做最小
   锁文件改动（`npm update source-map-js` 或等效），不升其他包；`npm
   audit` 复核该项消失。
5. **B-10 孤儿草稿：只做“检测 + 仅本人可见”，不做任何远端或本地自动
   清理/删除。**新增本地“草稿创建尝试”记录（远端创建前写入，成功记录后
   标记完成，不存正文/秘密），本人可列出自己未对账的尝试；不向 Shared
   Mailbox 其他成员披露；新增独立审计事件，不复用 `mail.drafts.queried`。
   远端 Stalwart 查询与清理继续不做。
6. **B-07 失败登录账本：做最小、独立、有界的控制面账本。**只记录原因码、
   时间、（可选）加盐哈希后的来源标识，绝不记录 code/state/id_token/
   Cookie/Token/PKCE；默认保留 30 天并自动修剪；不进租户审计，不暴露给
   租户管理端 UI，只供平台运维（CLI/只读指标）。设备列表只显示本人已
   持久化的非秘密会话属性，不对接 IdP 设备会话。
7. **B-05 IdP Back-Channel Logout：现在不实现**（缺 IdP 端点与安全事件
   契约）。保留 B-16 本地会话撤销作为唯一撤销路径；文档注明边界。
8. **批次 5 的 5 条：**
   - 第 1 条（复查返回到 COMMIT 前窗口）维持现状并在文档中明示：把复查
     升级为 `FOR UPDATE` 有死锁风险，不加。
   - 第 2 条（SCIM 提交前复查不要求 Actor active）维持。
   - 第 3 条（安全方法仅有会话允许列表时可过 CSRF）维持，并补一个“生产
     模式会话允许列表与 OIDC relying party 必须同时配置，否则启动失败”
     的配置校验（已落地为
     `copilotmail-platform` 的 `validate_human_session_oidc_pairing`）。
   - 第 4、5 条已在批次 9/10 解决。
9. **B-21 跨数据库选主：不实现。**维持同集群 PostgreSQL 会话级 advisory
   lock 单活加 [ADR 0025](adr/0025-recoverable-single-tenant-audit-maintenance.md)
   要求的外部调度器隔离；仅补文档/运维说明。
10. **推送：用户允许把经复核的新提交以普通 push（不 force）推到
    `origin/grok/dev`；不建 PR，不碰其他分支。**
