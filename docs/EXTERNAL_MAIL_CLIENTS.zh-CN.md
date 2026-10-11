# 外部邮件客户端与应用发信

> 当前只完善代码和测试，不进行部署。本文件不提供一个已经上线的主机或可用账号。
> “外部邮箱软件”按第三方邮件客户端及应用的 SMTP 库理解；验证码业务留在调用应用。

## 1. 接入约定

| 用途 | 标准协议/端口 | TLS 模式 | 开发覆盖层已有端口 |
| --- | --- | --- | --- |
| 客户端/应用发信 | SMTP Submission 465 | 连接时立即 TLS（SMTPS） | 2465 |
| 客户端/应用发信 | SMTP Submission 587 | 必须成功 STARTTLS 后才 AUTH | 2587 |
| 客户端收信 | IMAP 993 | 连接时立即 TLS（IMAPS） | 2993 |
| 兼容性收信测试 | IMAP 143 | 必须成功 STARTTLS 后才登录 | 2143 |
| 邮件服务器间入站 | SMTP relay 25 | 与客户端 Submission 分离 | 2525 |

服务器地址、账号和实际开放端口以后由批准的运行配置提供，不能把开发映射当成生产地址。
客户端使用其获授权的普通邮箱凭据；Stalwart 管理 API Key、CopilotMail Admin Cookie 和 Agent
Bearer 都不能当作普通 SMTP/IMAP 密码。应用可使用专用发信账户，并限制其实际需要的能力。

客户端连接优先考虑隐式 TLS，兼容 STARTTLS 时必须拒绝降级，并校验证书和主机名。
邮件提交/访问的 TLS 1.2 或更新版本及隐式 TLS 约定依据
[RFC 8314](https://datatracker.ietf.org/doc/html/rfc8314)。代码分别使用 Python 标准库
[SMTP/SMTP_SSL](https://docs.python.org/3/library/smtplib.html) 和
[IMAP4/IMAP4_SSL](https://docs.python.org/3/library/imaplib.html)。

## 2. 代码侧已经补齐的验证能力

`tests/stalwart_poc.py` 保留既有命令，新增两个可选配置：

| 环境变量 | 值 | 未显式指定端口时的选择 |
| --- | --- | --- |
| `COPILOTMAIL_STALWART_SMOKE_SUBMISSION_TLS_MODE` | `starttls`（默认）或 `implicit_tls` | 2587 或 2465 |
| `COPILOTMAIL_STALWART_SMOKE_IMAP_TLS_MODE` | `starttls`（默认）或 `implicit_tls` | 2143 或 2993 |

原端口变量仍可覆盖这些默认值；TLS 模式不通过端口号猜测。`implicit_tls` 失败不会尝试 STARTTLS，
STARTTLS 失败不会继续明文登录。正常输出记录所选模式，不输出密码。

连接模块先完成 TLS，再由现有脚本执行普通账号 AUTH/登录。IMAP 读回仍使用精确 Message-ID、
只读 INBOX 和 `BODY.PEEK[]`，不通过测试污染人的已读状态。JMAP 任意重定向在转发之前被拒绝，
不是携带 Authorization 跳过去后才报错。外部服务器返回的异常文本不会直接进入命令输出。

无需服务器的本地验证命令：

```powershell
py -3.13 tests/stalwart_poc.py self-test
py -3.13 -m unittest discover -s tests -p test_mail_client_transport.py -v
```

第二条覆盖两种 SMTP/IMAP TLS 模式、握手失败清理、证书失败无降级、认证时序、隐式端口选择、
只读读回、秘密输出与 IMAP 查询注入拒绝。网络构造器及 socket/DNS 均被测试替身拦截，不能
把这些离线结果描述成真实 Stalwart、特定客户端软件或公网邮件已经验收。

需要实际连接服务器的 `preflight/send/verify/roundtrip` 命令保留，但本阶段不执行。
后续受控运行方式仍见 [Stalwart PoC](STALWART_POC.zh-CN.md)。

## 3. 不能忽略的治理边界

普通客户端直接连接邮件引擎，不会自动经过 CopilotMail 的 Human/Agent HTTP 用例。因此：

- 不能声称 SMTP 直接发送自动继承了 Agent 的准入、首次联系人审批、额度或幂等检查。
  受控 Agent 继续走既有 Agent API；未建立邮件引擎侧等价强制点前，不向其开放绕过用的直连凭据。
- 引擎日志、入站/队列事件和控制面的同事务审计不同；必须明确各自证据来源，不能补造一条
  并不存在的 CopilotMail submission 记录或通过地址/主题/时间猜对应关系。
- 当前 Human Web 的 Sent 目录只显示本人经过控制面接受的提交。外部客户端直接发送/复制到
  引擎 Sent 文件夹的邮件，不保证出现在这个最小目录中；统一读模型仍是后续功能，不能假装已支持。
- 外部客户端修改 IMAP 已读状态不能变成 Agent Ack；账号开通、停用/轮换、From 限制和权限回收
  的引擎侧联动必须在代码/真实依赖验收中分别证明。
- 当前只有 Stalwart adapter；本文不表示已经支持把任意外部邮箱供应商作为新数据平面。

## 4. 业务应用的验证码责任

应用自己产生与校验验证码，将验证码放入邮件正文后通过标准 SMTP 库发送即可。
平台不新增验证码数据库、校验 API、验证收据或对应管理页面。
SMTP 发送超时可能发生在服务器已经接受之后，调用应用不得把网络异常当成一定未发出；应遵循
自己的业务幂等与恢复规则，避免反复发送。邮件平台不自动登录或修改应用用户的账户。
