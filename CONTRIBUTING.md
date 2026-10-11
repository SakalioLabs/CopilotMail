# Contributing to CopilotMail / 参与 CopilotMail 开发

CopilotMail is being built as an enterprise control plane, not as a collection
of isolated mail features. Changes must preserve tenant isolation, default-deny
authorization, Human/Agent state separation, and append-only audit evidence.

CopilotMail 按企业控制平面建设，而不是零散邮件功能的集合。所有变更都必须保持
租户隔离、默认拒绝授权、Human/Agent 状态隔离以及追加式审计证据。

## Before changing code / 修改代码前

1. Read [the project goal](GOAL.md), the
   [implementation plan](docs/IMPLEMENTATION_PLAN.zh-CN.md), and the
   [current implementation status](docs/IMPLEMENTATION_STATUS.zh-CN.md).
2. Check the relevant ADR and threat-model entry before changing a trust
   boundary.
3. Never put credentials, tokens, private keys, production addresses, or
   customer data in source, fixtures, logs, screenshots, or issue text.
4. Treat existing worktree changes as owned by their author. Do not discard
   unrelated changes.

## Architecture rules / 架构规则

- Domain crates do not depend on Axum, SQLx, HTTP, or provider implementations.
- Application use cases own business rules and define ports.
- PostgreSQL, object storage, identity, DNS, Webhook, and mail-engine crates
  implement those ports.
- Interfaces authenticate first, authorize against the live tenant state, and
  reject ambiguous credentials.
- Privileged state changes, their audit event, and their Outbox event commit in
  the same PostgreSQL transaction.
- Remote network I/O is not held inside a database transaction. Persist exact
  result evidence after the remote operation and provide explicit recovery
  semantics.
- Human message state and Agent processing state must never be conflated.

## Database migrations / 数据库迁移

- Add a new, monotonically numbered migration; never edit a migration already
  applied to a shared environment.
- Use explicit constraints and tenant keys. Security invariants belong in both
  the application and PostgreSQL where possible.
- Immutable evidence tables reject `UPDATE`, `DELETE`, and `TRUNCATE`.
- A new governed action must stay identical in the domain audit catalog,
  migration catalog insertion, and application producer literal.
- Add upgrade, rerun, rollback-safety, and negative tests appropriate to the
  migration.

## Local verification / 本地验证

The repository pins Rust in `rust-toolchain.toml` and Node in CI. Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python scripts/check_architecture.py --self-test
python scripts/check_crypto_boundary.py --self-test
python scripts/check_audit_governance.py
python -m unittest discover -s scripts -p test_audit_governance.py
python -m pip install -r scripts/requirements-quality.txt
python scripts/check_postgres_syntax.py --self-test
python scripts/check_agent_skill.py
python scripts/check_compose_security.py --self-test
python scripts/check_documentation.py
npm --prefix web ci
node web/scripts/build-quality-inventory.mjs
python scripts/check_function_test_matrix.py --self-test
python scripts/check_web_i18n.py --self-test

cd web
npm audit --audit-level=moderate
npm run check
npm run test:coverage
npm run build
npm run e2e:run
```

`check_architecture.py` is also enforced by CI. It evaluates the resolved graph
from the canonical Domain, Audit, and Application roots against their layer
allowlists and scans the complete Mail Engine Rust corpus so neutral ports remain
Application-owned and privately imported by the adapter.

`check_web_i18n.py` likewise rejects production Human Mail UI text that bypasses
the typed bilingual catalog.

PostgreSQL and MinIO tests marked `ignored` require disposable PostgreSQL 17
and Object-Lock-enabled S3-compatible storage. Follow
[the CI workflow](.github/workflows/ci.yml) rather than pointing destructive
tests at a shared database or bucket.

PG integration requires explicit `COPILOTMAIL_ALLOW_ISOLATED_DB_TESTS=1` and a
database named `copilotmail_test_<suffix>`; URI query/fragment overrides are
rejected. The documented CI loopback service is the only exception. Offline
SQL/PLpgSQL parsing never opens a database connection.

Source inventories, measured coverage and capability assertions are separate.
Use `cargo llvm-cov --workspace --locked --json --output-path
quality/coverage/rust-workspace.json` after installing the pinned CI tool and
creating the output directory, then `python scripts/build_quality_inventory.py
--llvm-report`. Do not mark initial matrices complete or ignored tests passed.
Read [the quality review](docs/CODE_QUALITY_REVIEW.zh-CN.md) before release.

Dependency checks include `cargo audit` (CI tool version 0.22.2). Do not add
blanket advisory ignores. The current unsuppressed RSA advisory and the
network-signing mitigation are documented in that review; a clean build is
not a clean security scan.

## Pull requests / 合并请求

- Keep one coherent security or product slice per change.
- Explain Human and Agent semantics, authorization boundaries, failure and
  retry behavior, and audit coverage.
- Update OpenAPI/AsyncAPI, migrations, ADRs, runbooks, and localized UI text in
  the same change.
- State exactly which automated and runtime checks were executed. Never present
  a compiled-but-ignored integration test as runtime evidence.
- Critical or High security findings remain release blockers until resolved or
  formally accepted by an authorized company owner.
