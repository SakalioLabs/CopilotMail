# Security Policy / 安全策略

CopilotMail handles authentication, authorization, mail content, and compliance
evidence. Please treat suspected vulnerabilities as sensitive.

CopilotMail 涉及身份认证、授权、邮件内容和合规证据。请将疑似安全问题按敏感信息
处理。

## Supported scope / 支持范围

- The V2 Rust workspace, React applications, deployment manifests, migrations,
  and documented adapters are under active security maintenance.
- The V1 handwritten SMTP implementation is retained only as a regression
  prototype and is not a supported production mail data plane.
- This repository has not reached Enterprise GA. The remaining acceptance
  boundaries are authoritative in
  [the implementation status](docs/IMPLEMENTATION_STATUS.zh-CN.md).

## Reporting / 报告方式

Report vulnerabilities privately to the repository owner or the company's
approved security channel. Do not open a public issue for an unpatched
vulnerability.

请通过仓库责任人或公司批准的安全渠道私下报告漏洞。未修复漏洞不得提交到公开
Issue。

Include only the minimum information needed to reproduce the issue:

- affected version or commit;
- affected component and deployment topology;
- reproducible steps using synthetic data;
- expected and observed authorization or isolation behavior;
- impact and any known safe mitigation.

Do not send production credentials, access tokens, private keys, mailbox
content, personal data, full database dumps, or unredacted logs. If secure
artifact transfer is required, agree on an approved channel first.

## Response and disclosure / 响应与披露

The maintainer will acknowledge through the same private channel, classify the
finding, and coordinate remediation and disclosure. No fixed response SLA is
claimed until the company approves the SLO and incident-response inputs in
[the enterprise checklist](docs/ENTERPRISE_INPUT_CHECKLIST.zh-CN.md).

Critical and High findings block release unless an authorized company owner
records a time-bounded risk acceptance. Coordinated disclosure must wait until
a fix or effective mitigation is available to affected deployments.

## Security baseline / 安全基线

- Default deny across tenant, RBAC, Token Scope, mailbox, and content
  boundaries.
- Human browser sessions and Agent credentials are distinct trust paths.
- Privileged state changes and their audit/Outbox evidence commit atomically.
- Secrets and message content are excluded from logs and audit details.
- Production ingress uses TLS, bounded inputs, no redirects across trust
  boundaries, and SSRF-resistant resolution.
- Migrations and deployment checks fail closed when required controls are
  missing or disabled.

See [the threat model](docs/THREAT_MODEL.zh-CN.md) and
[authorization matrix](docs/AUTHORIZATION_MATRIX.zh-CN.md) for the maintained
control inventory.
