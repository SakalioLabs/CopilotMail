# 已撤回设计原型归档

2026-09-19 用户澄清：验证码由业务应用处理，邮件平台只提供收发能力；当前不部署。

[验证码领域原型归档](verification-domain-prototype-2026-09-19.zip) 保留本轮曾新增的六个 Rust
文件，便于追踪和恢复。SHA-256：

```text
8DEB3ADD0159D0725AC8DCE0773BFBA64F5F663CEB50CCF6ECBDF2973352EE4B
```

这些文件已经移出 Cargo 编译图；专属的 `ApplicationId`、`VerificationChallengeId` 和
`VerificationReceiptId` 及当前测试矩阵引用也已撤下。原型从未接入 HTTP、PG migration、
真实发信或用户数据，所以不需要回滚任何线上数据库。

归档不是独立可编译 crate；如未来重新批准该需求，只应解压到单独评审目录，重新评审三个 ID、
导出、策略、接口、事务和测试后再决定接入，不自动覆盖当前源文件。
此前 12 项测试与 344 项 Rust 总数保留为当时历史记录，不作为当前功能承诺。
